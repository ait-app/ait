import { BrowserWindow, dialog, type WebContents } from "electron";
import { randomUUID } from "node:crypto";
import { open, rename, unlink } from "node:fs/promises";
import path from "node:path";
import { WebSocket } from "ws";
import type { AccountSessionManager } from "./account-session.js";

/** Streams each independent download to a user-selected file with bounded memory. */
export class AccountDownloadManager {
  private active = new Set<() => void>();
  constructor(private readonly account: AccountSessionManager) {}
  closeAll(): void {
    for (const cancel of this.active) cancel();
  }

  async download(
    owner: WebContents,
    input: { hostId: string; token: string; fileName: string; downloadId: string },
  ): Promise<void> {
    if (this.active.size >= 4) throw new Error("Too many concurrent downloads.");
    let aborted = false;
    let cancelTransfer: () => void = () => undefined;
    const reservation = () => {
      aborted = true;
      cancelTransfer();
    };
    const navigated = (
      _event: unknown,
      _url: string,
      sameDocument: boolean,
      mainFrame: boolean,
    ) => {
      if (mainFrame && !sameDocument) reservation();
    };
    this.active.add(reservation);
    owner.on("did-start-navigation", navigated);
    owner.once("destroyed", reservation);
    try {
      await this.transfer(
        owner,
        input,
        () => aborted,
        (value) => {
          cancelTransfer = value;
        },
      );
    } finally {
      this.active.delete(reservation);
      owner.removeListener("did-start-navigation", navigated);
      owner.removeListener("destroyed", reservation);
    }
  }

  private async transfer(
    owner: WebContents,
    input: { hostId: string; token: string; fileName: string; downloadId: string },
    isAborted: () => boolean,
    setCancel: (cancel: () => void) => void,
  ): Promise<void> {
    const window = BrowserWindow.fromWebContents(owner);
    if (!window) throw new Error("The download window has closed.");
    const selected = this.account.snapshot().selected?.host_id;
    if (selected !== input.hostId) throw new Error("Connect to the target host first.");
    const fileName =
      path
        .basename(input.fileName)
        // eslint-disable-next-line no-control-regex -- Filenames must exclude ASCII control characters.
        .replace(/[\\/:*?"<>|\x00-\x1f]/g, "_")
        .slice(0, 200) || "download";
    const destination = await dialog.showSaveDialog(window, { defaultPath: fileName });
    if (isAborted() || owner.isDestroyed() || destination.canceled || !destination.filePath)
      throw new Error("Download cancelled.");
    const grant = await this.account.openDownload(input.hostId, input.token);
    const temporary = `${destination.filePath}.ait-${randomUUID()}.part`;
    if (isAborted()) {
      await this.account.closeVisit(grant.relay_session_id);
      throw new Error("Download cancelled.");
    }
    const file = await open(temporary, "wx", 0o600).catch(async (error) => {
      await this.account.closeVisit(grant.relay_session_id);
      throw error;
    });
    let completed = false;
    let cancel: () => void = () => undefined;
    try {
      if (isAborted()) throw new Error("Download cancelled.");
      await new Promise<void>((resolve, reject) => {
        const socket = new WebSocket(grant.url, {
          headers: { Authorization: `Bearer ${grant.client_ticket}` },
          maxPayload: 1024 * 1024,
          perMessageDeflate: false,
          handshakeTimeout: 5000,
        });
        let settled = false;
        let stage: "pairing" | "headers" | "body" | "done" = "pairing";
        let bytes = 0;
        let total: number | null = null;
        let pendingBytes = 0;
        let pendingMessages = 0;
        let lastProgress = 0;
        let work = Promise.resolve();
        let timer = setTimeout(() => finish(new Error("Download connection timed out.")), 30_000);
        const finish = (error?: Error) => {
          if (settled) return;
          settled = true;
          clearTimeout(timer);
          socket.terminate();
          owner.removeListener("destroyed", cancel);
          // Drain the current bounded disk write before the caller closes/removes the file.
          void work
            .catch(() => undefined)
            .then(() => {
              if (error) reject(error);
              else resolve();
            });
        };
        cancel = () => finish(new Error("Download cancelled."));
        setCancel(cancel);
        owner.once("destroyed", cancel);
        socket.on("error", () => finish(new Error("Download connection failed.")));
        socket.on("close", () => {
          if (stage !== "done") finish(new Error("Download interrupted."));
        });
        socket.on("message", (raw, binary) => {
          if (settled) return;
          const chunk = Buffer.isBuffer(raw)
            ? raw
            : raw instanceof ArrayBuffer
              ? Buffer.from(raw)
              : Buffer.concat(raw);
          pendingBytes += chunk.length;
          pendingMessages += 1;
          socket.pause();
          if (pendingBytes > 4 * 1024 * 1024 || pendingMessages > 16) {
            finish(new Error("Download buffer is full."));
            return;
          }
          clearTimeout(timer);
          timer = setTimeout(() => finish(new Error("Download stalled.")), 30_000);
          work = work
            .then(async () => {
              if (settled) return;
              if (binary) {
                if (stage !== "body") throw new Error("Invalid download protocol.");
                let offset = 0;
                while (offset < chunk.length) {
                  const result = await file.write(chunk, offset, chunk.length - offset);
                  if (!result.bytesWritten) throw new Error("Failed to write the file.");
                  offset += result.bytesWritten;
                }
                bytes += chunk.length;
                if (total !== null && bytes > total) throw new Error("File size mismatch.");
                if (Date.now() - lastProgress > 200 && !owner.isDestroyed()) {
                  lastProgress = Date.now();
                  owner.send("paseo:event:account-download-progress", {
                    id: input.downloadId,
                    bytesWritten: bytes,
                    totalBytes: total ?? 0,
                  });
                }
                return;
              }
              const message = JSON.parse(chunk.toString()) as {
                type?: string;
                relay_session_id?: string;
                content_length?: number | null;
                status?: number;
                bytes?: number;
              };
              if (
                stage === "pairing" &&
                message.type === "relay.ready" &&
                message.relay_session_id === grant.relay_session_id
              ) {
                stage = "headers";
                return;
              }
              if (
                stage === "headers" &&
                message.type === "download.headers" &&
                message.status === 200
              ) {
                if (
                  message.content_length !== null &&
                  (!Number.isSafeInteger(message.content_length) || message.content_length! < 0)
                )
                  throw new Error("Invalid file size.");
                total = message.content_length ?? null;
                stage = "body";
                return;
              }
              if (
                stage === "body" &&
                message.type === "download.end" &&
                message.bytes === bytes &&
                (total === null || total === bytes)
              ) {
                await file.sync();
                await new Promise<void>((resolve, reject) =>
                  socket.send(JSON.stringify({ type: "download.complete" }), (error) =>
                    error ? reject(error) : resolve(),
                  ),
                );
                stage = "done";
                queueMicrotask(() => finish());
                return;
              }
              throw new Error("Invalid or incomplete download response.");
            })
            .catch((error) => {
              queueMicrotask(() =>
                finish(error instanceof Error ? error : new Error("Download failed.")),
              );
            })
            .finally(() => {
              pendingBytes -= chunk.length;
              pendingMessages -= 1;
              if (!settled && pendingMessages === 0) socket.resume();
            });
        });
      });
      await file.close();
      await rename(temporary, destination.filePath);
      completed = true;
    } finally {
      setCancel(() => undefined);
      await file.close().catch(() => undefined);
      if (!completed) await unlink(temporary).catch(() => undefined);
      await this.account.closeVisit(grant.relay_session_id);
    }
  }
}

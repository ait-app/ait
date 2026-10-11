import { BrowserWindow, dialog, type WebContents } from "electron";
import { randomUUID } from "node:crypto";
import { open, rename, unlink } from "node:fs/promises";
import path from "node:path";
import { WebSocket } from "ws";
import { normalizeCenter, type AccountSessionManager } from "./account-session.js";

interface PreparedDownload {
  owner: WebContents;
  hostId: string;
  center: string;
  downloadId: string;
  destination: string | null;
  started: boolean;
  aborted: boolean;
  cancelTransfer: () => void;
  cancel: () => void;
  dispose: () => void;
  timer?: ReturnType<typeof setTimeout>;
}

/** Streams each independent download to a user-selected file with bounded memory. */
export class AccountDownloadManager {
  private active = new Map<string, PreparedDownload>();
  constructor(private readonly account: AccountSessionManager) {}
  closeAll(): void {
    for (const download of this.active.values()) download.cancel();
  }

  async prepare(
    owner: WebContents,
    input: { hostId: string; center: string; fileName: string; downloadId: string },
  ): Promise<string> {
    if (this.active.size >= 4) throw new Error("Too many concurrent downloads.");
    const window = BrowserWindow.fromWebContents(owner);
    if (!window) throw new Error("The download window has closed.");
    const account = this.account.snapshot();
    const center = normalizeCenter(input.center);
    if (account.status === "logged_out" || account.center !== center)
      throw new Error("Sign in to the online service used by this host.");
    const id = randomUUID();
    const download: PreparedDownload = {
      owner,
      hostId: input.hostId,
      center,
      downloadId: input.downloadId,
      destination: null,
      started: false,
      aborted: false,
      cancelTransfer: () => undefined,
      cancel: () => {
        download.aborted = true;
        download.cancelTransfer();
        if (!download.started) download.dispose();
      },
      dispose: () => {
        clearTimeout(download.timer);
        this.active.delete(id);
        owner.removeListener("did-start-navigation", navigated);
        owner.removeListener("destroyed", download.cancel);
      },
    };
    const navigated = (
      _event: unknown,
      _url: string,
      sameDocument: boolean,
      mainFrame: boolean,
    ) => {
      if (mainFrame && !sameDocument) download.cancel();
    };
    this.active.set(id, download);
    owner.on("did-start-navigation", navigated);
    owner.once("destroyed", download.cancel);
    const fileName =
      path
        .basename(input.fileName)
        // eslint-disable-next-line no-control-regex -- Filenames must exclude ASCII control characters.
        .replace(/[\\/:*?"<>|\x00-\x1f]/g, "_")
        .slice(0, 200) || "download";
    try {
      const destination = await dialog.showSaveDialog(window, { defaultPath: fileName });
      if (download.aborted || owner.isDestroyed() || destination.canceled || !destination.filePath)
        throw new Error("Download cancelled.");
      download.destination = destination.filePath;
      // Bound abandoned preparations; time spent in the save dialog does not count.
      download.timer = setTimeout(download.cancel, 300_000);
      return id;
    } catch (error) {
      download.dispose();
      throw error;
    }
  }

  cancel(owner: WebContents, preparationId: string): void {
    const download = this.active.get(preparationId);
    if (download?.owner === owner) download.cancel();
  }

  async download(
    owner: WebContents,
    input: { preparationId: string; token: string },
  ): Promise<void> {
    const download = this.active.get(input.preparationId);
    if (!download || download.owner !== owner || download.started || !download.destination)
      throw new Error("Invalid download preparation.");
    download.started = true;
    clearTimeout(download.timer);
    try {
      await this.transfer(
        owner,
        {
          hostId: download.hostId,
          center: download.center,
          token: input.token,
          downloadId: download.downloadId,
        },
        download.destination,
        () => download.aborted,
        (value) => {
          download.cancelTransfer = value;
        },
      );
    } finally {
      download.dispose();
    }
  }

  private async transfer(
    owner: WebContents,
    input: { hostId: string; center: string; token: string; downloadId: string },
    destination: string,
    isAborted: () => boolean,
    setCancel: (cancel: () => void) => void,
  ): Promise<void> {
    if (isAborted() || owner.isDestroyed()) throw new Error("Download cancelled.");
    const grant = await this.account.openDownload(input.hostId, input.token, input.center);
    const temporary = `${destination}.ait-${randomUUID()}.part`;
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
      await rename(temporary, destination);
      completed = true;
    } finally {
      setCancel(() => undefined);
      await file.close().catch(() => undefined);
      if (!completed) await unlink(temporary).catch(() => undefined);
      await this.account.closeVisit(grant.relay_session_id);
    }
  }
}

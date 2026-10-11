import type { AccountSessionManager } from "@ait/client/internal/account-session";
import {
  createWebSocketTransportFactory,
  nativeWebSocketFactory,
} from "@ait/client/internal/daemon-client-websocket-transport";
import { getNativeAccount, registerNativeAccountTransport } from "../native-account";
import { object, type Transport, type TransportFactory } from "./types";

interface DownloadDependencies {
  account(): Promise<Pick<AccountSessionManager, "openDownload" | "closeVisit">>;
  register(close: () => void): () => void;
  connect: TransportFactory;
}

/** Write download frames directly to a native file without accumulating the file in JS memory. */
export function streamNativeAccountDownload(
  input: {
    hostId: string;
    center: string;
    token: string;
    write(bytes: Uint8Array): void;
    progress(bytesWritten: number, totalBytes: number): void;
  },
  deps: DownloadDependencies = {
    account: getNativeAccount,
    register: registerNativeAccountTransport,
    connect: createWebSocketTransportFactory(nativeWebSocketFactory),
  },
): Promise<void> {
  return new Promise((resolve, reject) => {
    let closed = false;
    let socket: Transport | undefined;
    let account: Awaited<ReturnType<DownloadDependencies["account"]>> | undefined;
    let grant: Awaited<ReturnType<AccountSessionManager["openDownload"]>> | undefined;
    let stage: "pairing" | "headers" | "body" = "pairing";
    let bytes = 0;
    let total: number | null = null;
    let lastProgress = 0;
    const cleanup: (() => void)[] = [];
    let timer = setTimeout(() => finish(new Error("Download connection timed out.")), 30_000);
    function finish(error?: Error): void {
      if (closed) return;
      closed = true;
      clearTimeout(timer);
      for (const remove of cleanup) remove();
      socket?.close();
      if (grant) void account?.closeVisit(grant.relay_session_id).catch(() => undefined);
      if (error) reject(error);
      else resolve();
    }
    function receive(data: unknown, binary: boolean): void {
      if (closed) return;
      try {
        clearTimeout(timer);
        timer = setTimeout(() => finish(new Error("Download stalled.")), 30_000);
        if (binary) {
          const chunk =
            data instanceof ArrayBuffer
              ? new Uint8Array(data)
              : ArrayBuffer.isView(data)
                ? new Uint8Array(data.buffer, data.byteOffset, data.byteLength)
                : null;
          if (stage !== "body" || !chunk || chunk.length > 1024 * 1024)
            throw new Error("Invalid download frame.");
          if (total !== null && bytes + chunk.length > total)
            throw new Error("File size mismatch.");
          input.write(chunk);
          bytes += chunk.length;
          if (Date.now() - lastProgress > 200) {
            lastProgress = Date.now();
            input.progress(bytes, total ?? 0);
          }
          return;
        }
        if (typeof data !== "string" || data.length > 16384)
          throw new Error("Invalid download response.");
        const message = object(JSON.parse(data));
        if (
          stage === "pairing" &&
          message.type === "relay.ready" &&
          message.relay_session_id === grant?.relay_session_id
        ) {
          stage = "headers";
          return;
        }
        if (stage === "headers" && message.type === "download.headers" && message.status === 200) {
          if (
            message.content_length !== null &&
            (typeof message.content_length !== "number" ||
              !Number.isSafeInteger(message.content_length) ||
              message.content_length < 0)
          ) {
            throw new Error("Invalid file size.");
          }
          total = message.content_length as number | null;
          stage = "body";
          return;
        }
        if (
          stage === "body" &&
          message.type === "download.end" &&
          message.bytes === bytes &&
          (total === null || total === bytes)
        ) {
          input.progress(bytes, total ?? bytes);
          socket!.send(JSON.stringify({ type: "download.complete" }));
          finish();
          return;
        }
        throw new Error("Invalid or incomplete download response.");
      } catch (error) {
        finish(error instanceof Error ? error : new Error("Download failed."));
      }
    }
    void Promise.resolve()
      .then(async () => {
        cleanup.push(deps.register(() => finish(new Error("Download cancelled."))));
        account = await deps.account();
        if (closed) return;
        grant = await account.openDownload(input.hostId, input.token, input.center);
        if (closed) {
          await account.closeVisit(grant.relay_session_id);
          return;
        }
        socket = deps.connect({
          url: grant.url,
          headers: { Authorization: `Bearer ${grant.client_ticket}` },
        });
        cleanup.push(
          socket.onMessage(receive),
          socket.onError(() => finish(new Error("Download connection failed."))),
          socket.onClose(() => finish(new Error("Download interrupted."))),
        );
      })
      .catch((error: unknown) =>
        finish(error instanceof Error ? error : new Error("Download failed.")),
      );
  });
}

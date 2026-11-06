import type { AccountSessionManager } from "@ait/client/internal/account-session";
import type { HubSession } from "@ait/client/internal/hub";
import {
  createWebSocketTransportFactory,
  nativeWebSocketFactory,
} from "@ait/client/internal/daemon-client-websocket-transport";
import { getNativeAccount, registerNativeAccountTransport } from "../native-account";
import { object, type Transport, type TransportFactory } from "@ait/client/internal/protocol/types";

interface DownloadDependencies {
  account(): Promise<
    Pick<AccountSessionManager, "openDownload" | "closeVisit"> &
      Partial<Pick<AccountSessionManager, "openHubDownload">>
  >;
  register(close: () => void): () => void;
  connect: TransportFactory;
}

/** After `download.complete`, wait this long for the Host's `done` before finishing anyway. */
const DONE_WAIT_MS = 10_000;

/**
 * Write download frames directly to a native file without accumulating the file in JS memory.
 * When the Host and this link both support Hub downloads the file streams over a Hub session;
 * otherwise the legacy `/relay-downloads` route carries the same messages.
 */
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
    let hub: HubSession | null = null;
    let account: Awaited<ReturnType<DownloadDependencies["account"]>> | undefined;
    let grant: Awaited<ReturnType<AccountSessionManager["openDownload"]>> | undefined;
    // A Hub session has no relay pairing; its first message is `download.headers`.
    let stage: "pairing" | "headers" | "body" | "done" = "pairing";
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
      hub?.end(error ? "cancelled" : "closed");
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
          // Hub downloads grant credit only for bytes already written.
          hub?.consumed(chunk.length);
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
          if (hub) {
            hub.send(JSON.stringify({ type: "download.complete" }));
            // The Host acknowledges with close `done`; the file is complete either way.
            stage = "done";
            clearTimeout(timer);
            timer = setTimeout(() => finish(), DONE_WAIT_MS);
            return;
          }
          socket!.send(JSON.stringify({ type: "download.complete" }));
          finish();
          return;
        }
        throw new Error("Invalid or incomplete download response.");
      } catch (error) {
        finish(error instanceof Error ? error : new Error("Download failed."));
      }
    }
    async function relay(): Promise<void> {
      grant = await account!.openDownload(input.hostId, input.token, input.center);
      if (closed) {
        await account!.closeVisit(grant.relay_session_id);
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
    }
    function attach(session: HubSession): void {
      hub = session;
      stage = "headers";
      cleanup.push(
        session.onMessage(receive),
        session.onClose((event) => {
          if (closed) return;
          if (stage === "done") {
            finish();
            return;
          }
          // The Host lost the feature after the hosts snapshot: use the relay route.
          if ((event as { hubCode?: string } | undefined)?.hubCode === "unsupported") {
            if (stage === "headers") {
              hub = null;
              stage = "pairing";
              relay().catch((error: unknown) =>
                finish(error instanceof Error ? error : new Error("Download failed.")),
              );
              return;
            }
          }
          finish(new Error("Download interrupted."));
        }),
      );
    }
    void Promise.resolve()
      .then(async () => {
        cleanup.push(deps.register(() => finish(new Error("Download cancelled."))));
        account = await deps.account();
        if (closed) return;
        const session =
          (await account.openHubDownload?.(input.hostId, input.token, input.center)) ?? null;
        if (closed) session?.end("cancelled");
        else if (session) attach(session);
        else await relay();
      })
      .catch((error: unknown) =>
        finish(error instanceof Error ? error : new Error("Download failed.")),
      );
  });
}

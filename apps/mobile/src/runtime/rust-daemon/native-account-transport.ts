import type { AccountSessionManager } from "@ait/client/internal/account-session";
import {
  createWebSocketTransportFactory,
  nativeWebSocketFactory,
} from "@ait/client/internal/daemon-client-websocket-transport";
import { getNativeAccount, registerNativeAccountTransport } from "../native-account";
import { object, type Transport, type TransportFactory } from "./types";

interface Dependencies {
  account(): Promise<Pick<AccountSessionManager, "openVisit" | "closeVisit">>;
  register(close: () => void): () => void;
  connect: TransportFactory;
}

/** Native WSS carries single-use tickets in headers; pairing precedes the Rust hello. */
export function createNativeAccountRelayTransportFactory(
  deps: Dependencies = {
    account: getNativeAccount,
    register: registerNativeAccountTransport,
    connect: createWebSocketTransportFactory(nativeWebSocketFactory),
  },
): TransportFactory {
  return ({ url }) => {
    const target = new URL(url);
    if (
      target.protocol !== "ait+desktop:" ||
      target.hostname !== "account-relay" ||
      !/^\/[0-9a-f-]{36}$/i.test(target.pathname) ||
      target.search ||
      target.hash ||
      target.username ||
      target.password
    ) {
      throw new Error("Invalid account relay target.");
    }
    const opens = new Set<() => void>();
    const closes = new Set<(event?: unknown) => void>();
    const errors = new Set<(event?: unknown) => void>();
    const messages = new Set<(data: unknown, binary: boolean) => void>();
    const cleanup: (() => void)[] = [];
    let socket: Transport | undefined;
    let account: Awaited<ReturnType<Dependencies["account"]>> | undefined;
    let grant: Awaited<ReturnType<AccountSessionManager["openVisit"]>> | undefined;
    let stage: "pairing" | "hello" | "ready" = "pairing";
    let closed = false;
    let helloSent = false;
    let timer = setTimeout(() => finish(new Error("Relay setup timed out.")), 45_000);

    function dispose(): boolean {
      if (closed) return false;
      closed = true;
      clearTimeout(timer);
      for (const remove of cleanup) remove();
      socket?.close();
      if (grant) void account?.closeVisit(grant.relay_session_id).catch(() => undefined);
      return true;
    }
    function finish(error?: Error): void {
      if (!dispose()) return;
      if (error) for (const handler of errors) handler(error);
      for (const handler of closes) handler({ code: error ? 1006 : 1000, reason: error?.message });
    }
    function receive(data: unknown, binary: boolean): void {
      if (closed) return;
      try {
        const length =
          typeof data === "string"
            ? new TextEncoder().encode(data).byteLength
            : data instanceof ArrayBuffer || ArrayBuffer.isView(data)
              ? data.byteLength
              : Infinity;
        if (length > 1024 * 1024) throw new Error("Relay frame exceeds the size limit.");
        if (stage !== "ready") {
          if (binary || typeof data !== "string") throw new Error("Invalid relay handshake.");
          const message = object(JSON.parse(data));
          if (stage === "pairing") {
            if (
              message.type !== "relay.ready" ||
              message.relay_session_id !== grant?.relay_session_id
            ) {
              throw new Error("Relay pairing rejected.");
            }
            stage = "hello";
            clearTimeout(timer);
            timer = setTimeout(() => finish(new Error("AIT hello timed out.")), 10_000);
            for (const handler of opens) handler();
            return;
          }
          const info = object(message.info);
          if (
            !helloSent ||
            message.type !== "server_info" ||
            info.server_id !== grant?.server_id ||
            info.instance_id !== grant?.instance_id ||
            !Array.isArray(info.features) ||
            !info.features.includes("ait-rust-single-v1")
          ) {
            throw new Error("Target host identity or protocol changed.");
          }
          stage = "ready";
          clearTimeout(timer);
        }
        for (const handler of messages) handler(data, binary);
      } catch (error) {
        finish(error instanceof Error ? error : new Error("Invalid relay response."));
      }
    }

    void Promise.resolve()
      .then(async () => {
        if (closed) return;
        cleanup.push(deps.register(() => finish()));
        account = await deps.account();
        if (closed) return;
        grant = await account.openVisit(target.pathname.slice(1));
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
          socket.onError(() => finish(new Error("Relay connection failed."))),
          socket.onClose(() => finish(new Error("Relay connection closed."))),
        );
      })
      .catch((error: unknown) =>
        finish(error instanceof Error ? error : new Error("Relay setup failed.")),
      );

    return {
      send(data) {
        if (closed || !socket || stage === "pairing") throw new Error("Relay is not ready.");
        const length =
          typeof data === "string" ? new TextEncoder().encode(data).byteLength : data.byteLength;
        if (length > 1024 * 1024) {
          finish(new Error("Relay frame exceeds the size limit."));
          throw new Error("Relay frame too large.");
        }
        if (stage === "hello") {
          if (helloSent || typeof data !== "string" || object(JSON.parse(data)).type !== "hello") {
            throw new Error("Expected the AIT hello before sending data.");
          }
          helloSent = true;
        }
        socket.send(data);
      },
      close() {
        dispose();
      },
      onOpen(handler) {
        opens.add(handler);
        return () => {
          opens.delete(handler);
        };
      },
      onClose(handler) {
        closes.add(handler);
        return () => {
          closes.delete(handler);
        };
      },
      onError(handler) {
        errors.add(handler);
        return () => {
          errors.delete(handler);
        };
      },
      onMessage(handler) {
        messages.add(handler);
        return () => {
          messages.delete(handler);
        };
      },
    };
  };
}

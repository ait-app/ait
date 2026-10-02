import { getDesktopHost } from "@/desktop/host";
import type { TransportFactory } from "./types";

/** Account authority and one-use relay tickets stay in the Electron main process. */
export const createAccountRelayTransportFactory: TransportFactory = ({ url }) => {
  const desktop = getDesktopHost();
  if (!desktop?.invoke || !desktop.events?.on)
    throw new Error("Account relay requires the desktop app.");
  const parsed = new URL(url);
  if (
    parsed.protocol !== "ait+desktop:" ||
    parsed.hostname !== "account-relay" ||
    !/^\/[0-9a-f-]{36}$/i.test(parsed.pathname) ||
    parsed.search ||
    parsed.hash
  )
    throw new Error("Invalid account relay target");
  const invoke = desktop.invoke;
  const sessionId = `account-${crypto.randomUUID()}`;
  const opens = new Set<() => void>();
  const closes = new Set<(event?: unknown) => void>();
  const errors = new Set<(event?: unknown) => void>();
  const messages = new Set<(data: unknown, binary: boolean) => void>();
  let disposed = false;
  let cleanup: (() => void) | undefined;
  const fail = (error: unknown) => {
    if (!disposed) for (const handler of errors) handler(error);
  };
  void (async () => {
    const remove = await desktop.events!.on!("account-relay-transport", (raw) => {
      const event = raw as {
        sessionId: string;
        kind: string;
        text?: string;
        binaryBase64?: string;
        sequence?: number;
        error?: string;
      };
      if (event.sessionId !== sessionId || disposed) return;
      if (event.kind === "open") {
        for (const handler of opens) handler();
        return;
      }
      if (event.kind === "close") {
        for (const handler of closes) handler({ code: 1006, reason: event.error });
        return;
      }
      if (event.kind === "message") {
        try {
          if (typeof event.text === "string")
            for (const handler of messages) handler(event.text, false);
          else if (event.binaryBase64 !== undefined) {
            const bytes = Uint8Array.from(atob(event.binaryBase64), (value) => value.charCodeAt(0));
            for (const handler of messages) handler(bytes, true);
          }
        } finally {
          void invoke("account_transport_ack", { sessionId, sequence: event.sequence }).catch(fail);
        }
      }
    });
    if (disposed) {
      remove();
      return;
    }
    cleanup = remove;
    await invoke("account_transport_open", { sessionId, hostId: parsed.pathname.slice(1) });
  })().catch(fail);
  return {
    send(data) {
      if (disposed) throw new Error("Relay connection closed");
      if (typeof data === "string")
        void invoke("account_transport_send", { sessionId, text: data }).catch(fail);
      else {
        const bytes = data instanceof ArrayBuffer ? new Uint8Array(data) : data;
        // Chunk conversion avoids argument-list limits on large binary file frames.
        let text = "";
        for (let i = 0; i < bytes.length; i += 8192)
          text += String.fromCharCode(...bytes.subarray(i, i + 8192));
        void invoke("account_transport_send", { sessionId, binaryBase64: btoa(text) }).catch(fail);
      }
    },
    close() {
      if (disposed) return;
      disposed = true;
      cleanup?.();
      void invoke("account_transport_close", { sessionId }).catch(() => undefined);
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

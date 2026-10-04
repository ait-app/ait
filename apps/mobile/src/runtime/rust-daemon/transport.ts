import { METHODS, type MethodSpec } from "./methods";
import { eventMessage, responseMessage, rpcError, serverInfo } from "./messages";
import { object, strings, type Payload, type Transport, type TransportFactory } from "./types";
import {
  asUint8Array,
  decodeFileTransferFrame,
  FileTransferOpcode,
} from "@ait/protocol/binary-frames/index";

const RUNTIME_METHODS: Readonly<Record<string, MethodSpec>> = {
  ...METHODS,
  checkout_reset_workspace_request: {
    method: "checkout.reset_workspace.request",
    kind: "request",
    channel: 2,
    response: "checkout_reset_workspace_response",
  },
};

export const CHANNEL_CAPABILITIES = Array.from({ length: 4 }, (_, channel) => [
  ...new Set([
    "connection.ping",
    "subscription.release.request",
    ...Object.values(RUNTIME_METHODS)
      .filter((spec) => spec.channel === channel)
      .map((spec) => spec.method),
  ]),
]);

interface Pending {
  request: Payload;
  spec: MethodSpec;
  channel: number;
  rawPing: boolean;
  wire: string;
  retries: number;
  retryTimer?: ReturnType<typeof setTimeout>;
  timer: ReturnType<typeof setTimeout>;
}

/** Adapt the existing UI SDK to Rust v1.0, preserving physical subscription ownership. */
export function createRustDaemonTransportFactory(baseFactory: TransportFactory): TransportFactory {
  return ({ url, headers }) => {
    const parsed = new URL(url);
    const single = parsed.protocol === "ait+desktop:" && parsed.hostname === "account-relay";
    const capabilityGroups = single
      ? [[...new Set(CHANNEL_CAPABILITIES.flat())]]
      : CHANNEL_CAPABILITIES;
    // SSH uses the desktop IPC URL; the main process validates its target and
    // opens only the Rust /v1/ws endpoint inside the authenticated tunnel.
    const ssh =
      parsed.protocol === "ait+desktop:" &&
      parsed.hostname === "ssh" &&
      (parsed.pathname === "" || parsed.pathname === "/");
    if (
      (!ssh && !single && !/^(ws|wss):$/.test(parsed.protocol)) ||
      (!ssh && !single && parsed.pathname !== "/v1/ws") ||
      parsed.username ||
      parsed.password ||
      (!ssh && parsed.search) ||
      parsed.hash
    ) {
      throw new Error("Invalid Rust daemon WebSocket endpoint");
    }
    const openHandlers = new Set<() => void>();
    const closeHandlers = new Set<(event?: unknown) => void>();
    const errorHandlers = new Set<(event?: unknown) => void>();
    const messageHandlers = new Set<(data: unknown, binary: boolean) => void>();
    const channels: Transport[] = [];
    const cleanup: (() => void)[] = [];
    const opened = new Set<number>();
    const negotiated = new Map<number, Set<string>>();
    const subscriptions = new Map<string, number>();
    const pending = new Map<string, Pending>();
    let disposed = false;
    let ready = false;
    let helloSent = false;
    let info: Payload | null = null;
    let implemented = new Set<string>();
    const setupTimer = setTimeout(
      () => fail(new Error("Rust daemon handshake timed out")),
      single ? 45_000 : 10_000,
    );

    function emit(value: Payload): void {
      if (!disposed) for (const handler of messageHandlers) handler(JSON.stringify(value), false);
    }

    function dispose(code = 1000, reason = "Client closed"): void {
      if (disposed) return;
      disposed = true;
      clearTimeout(setupTimer);
      for (const item of pending.values()) {
        clearTimeout(item.timer);
        clearTimeout(item.retryTimer);
      }
      pending.clear();
      subscriptions.clear();
      for (const remove of cleanup) remove();
      for (const channel of channels) channel.close(code, reason);
    }

    function fail(error: Error): void {
      if (disposed) return;
      dispose(1000, "Connection failed");
      for (const handler of errorHandlers) handler(error);
      for (const handler of closeHandlers) handler({ code: 1006, reason: error.message });
    }

    function finishPending(id: string, item: Pending): void {
      pending.delete(id);
      clearTimeout(item.timer);
      clearTimeout(item.retryTimer);
    }

    function receiveBinary(channel: number, data: unknown): void {
      const bytes = asUint8Array(data);
      if (!bytes?.length) throw new Error("Invalid Rust binary frame");
      if (bytes[0] < FileTransferOpcode.FileBegin) {
        for (const handler of messageHandlers) handler(data, true);
        return;
      }
      const frame = decodeFileTransferFrame(bytes);
      if (!frame) throw new Error("Invalid Rust file transfer frame");
      const item = pending.get(frame.requestId);
      if (!item) return; // Ignore late frames after a failed or completed read.
      if (item.channel !== channel) throw new Error("Rust file came from the wrong connection");
      if (item.spec.method !== "fs.explorer.request" || item.request.acceptBinary !== true)
        throw new Error("Unexpected Rust file transfer response");
      // Binary reads finish at FileEnd; the server does not also send a JSON response.
      if (frame.opcode === FileTransferOpcode.FileEnd) finishPending(frame.requestId, item);
      for (const handler of messageHandlers) handler(data, true);
    }

    function receive(channel: number, data: unknown, binary: boolean): void {
      if (disposed) return;
      if (binary) {
        if (!ready) throw new Error("Binary data received before Rust handshake");
        receiveBinary(channel, data);
        return;
      }
      const message = object(JSON.parse(String(data)));
      if (message.type === "server_info") {
        if (!helloSent || negotiated.has(channel)) throw new Error("Unexpected Rust daemon hello");
        const nextInfo = object(message.info);
        const protocol = object(nextInfo.protocol);
        if (
          protocol.major !== 1 ||
          protocol.minor !== 0 ||
          typeof nextInfo.server_id !== "string"
        ) {
          throw new Error("Unsupported Rust daemon protocol");
        }
        if (
          info &&
          (info.server_id !== nextInfo.server_id || info.instance_id !== nextInfo.instance_id)
        ) {
          throw new Error("Rust daemon restarted during handshake");
        }
        info = nextInfo;
        implemented = new Set(strings(nextInfo.implemented_capabilities));
        negotiated.set(channel, new Set(strings(message.negotiated_capabilities)));
        if (negotiated.size === capabilityGroups.length) {
          ready = true;
          clearTimeout(setupTimer);
          emit(serverInfo(info, implemented));
        }
        return;
      }
      if (!ready)
        throw new Error(`Rust handshake rejected: ${String(message.code ?? message.type)}`);
      if (message.type === "event") {
        if (message.method === "status.server_info") {
          const params = object(message.params);
          const update = serverInfo(object(params.info), implemented);
          const payload = object(object(update.message).payload);
          if (typeof params.subscriptionId === "string")
            payload.subscriptionId = params.subscriptionId;
          emit(update);
          return;
        }
        emit(eventMessage(String(message.method), message.params));
        return;
      }
      if (message.type !== "response" && message.type !== "error") {
        throw new Error("Unexpected Rust daemon envelope");
      }
      const id = message.request_id;
      if (typeof id !== "string") {
        // An event rejection (for example a terminal resize) is an application error.
        // Transport onError makes the SDK reconnect every channel and replay the failing event.
        emit(
          rpcError(
            crypto.randomUUID(),
            channel === 1 ? "terminal_input" : "event",
            String(message.code),
            String(message.message ?? message.code),
          ),
        );
        return;
      }
      const item = pending.get(id);
      if (!item) return; // A timed-out request can complete after its SDK waiter is gone.
      if (item.channel !== channel) throw new Error("Rust response came from the wrong connection");
      // Admission failures have no side effects. Respect the server's retryability
      // instead of losing settings reads when the shared worker is briefly busy.
      if (
        message.type === "error" &&
        message.code === "resource_exhausted" &&
        message.retryable === true &&
        item.retries < 5
      ) {
        item.retryTimer = setTimeout(
          () => {
            item.retryTimer = undefined;
            if (disposed || pending.get(id) !== item) return;
            try {
              channels[channel].send(item.wire);
            } catch (error) {
              fail(error instanceof Error ? error : new Error("Rust request retry failed"));
            }
          },
          100 * 2 ** item.retries++,
        );
        return;
      }
      finishPending(id, item);
      if (message.type === "error") {
        emit(
          rpcError(id, String(item.request.type), String(message.code), String(message.message)),
        );
        return;
      }
      const result = object(message.result);
      if (typeof result.subscriptionId === "string")
        subscriptions.set(result.subscriptionId, channel);
      if (
        item.spec.method === "subscription.release.request" &&
        typeof item.request.subscriptionId === "string"
      ) {
        subscriptions.delete(item.request.subscriptionId);
      }
      if (item.rawPing) emit({ type: "pong" });
      else if (item.spec.response)
        emit(responseMessage(item.spec.response, id, result, item.request));
    }

    function request(message: Payload, rawPing = false): void {
      const name = String(message.type);
      const spec = RUNTIME_METHODS[name];
      if (!spec) throw new Error(`No Rust daemon method mapping for ${name}`);
      const callback = spec.kind === "response" ? object(message.payload) : undefined;
      const id =
        typeof callback?.requestId === "string"
          ? callback.requestId
          : typeof message.requestId === "string"
            ? message.requestId
            : crypto.randomUUID();
      const channel = single
        ? 0
        : spec.method === "subscription.release.request" &&
            typeof message.subscriptionId === "string"
          ? (subscriptions.get(message.subscriptionId) ?? spec.channel)
          : spec.channel;
      if (!negotiated.get(channel)?.has(spec.method) || !implemented.has(spec.method)) {
        const error = `Rust daemon does not implement ${spec.method}`;
        if (typeof message.requestId === "string") {
          emit(rpcError(id, name, "not_implemented", error));
          return;
        }
        throw new Error(error);
      }
      if (name === "ping" && !rawPing) {
        emit(
          rpcError(
            id,
            name,
            "unsupported_capability",
            "Rust daemon does not provide server-side ping timestamps",
          ),
        );
        return;
      }
      const { type: _type, requestId: _requestId, ...params } = message;
      // This requestId identifies a provider permission, not just the UI RPC waiter.
      if (name === "agent_permission_response") params.requestId = message.requestId;
      if (rawPing) params.nonce = id;
      if (spec.kind !== "request") {
        channels[channel].send(
          JSON.stringify({
            type: spec.kind,
            method: spec.method,
            params: callback ?? params,
            ...(spec.kind === "response" ? { request_id: id } : {}),
          }),
        );
        return;
      }
      if (pending.size >= 256) throw new Error("Too many pending Rust daemon requests");
      if (pending.has(id)) throw new Error("Duplicate pending Rust daemon request ID");
      const wire = JSON.stringify({
        type: "request",
        request_id: id,
        method: spec.method,
        params,
      });
      const timer = setTimeout(() => {
        const timedOut = pending.get(id);
        if (!pending.delete(id) || disposed) return;
        clearTimeout(timedOut?.retryTimer);
        emit(rpcError(id, name, "timeout", "Rust daemon request timed out"));
      }, 300_000);
      pending.set(id, {
        request: message,
        spec,
        channel,
        rawPing,
        wire,
        retries: 0,
        timer,
      });
      try {
        channels[channel].send(wire);
      } catch (error) {
        pending.delete(id);
        clearTimeout(timer);
        throw error;
      }
    }

    try {
      for (let index = 0; index < capabilityGroups.length; index += 1) {
        // Electron/native uses Bearer headers; browsers exchange them for one-use tickets.
        const channel = baseFactory({ url, headers });
        channels.push(channel);
        cleanup.push(
          channel.onOpen(() => {
            opened.add(index);
            if (opened.size === capabilityGroups.length && !disposed) {
              for (const handler of openHandlers) handler();
            }
          }),
          channel.onMessage((data, binary) => {
            try {
              receive(index, data, binary);
            } catch (error) {
              fail(error instanceof Error ? error : new Error("Invalid Rust daemon response"));
            }
          }),
          channel.onError((error) =>
            fail(
              error instanceof Error
                ? error
                : new Error("Rust daemon transport failed; check address and Bearer token"),
            ),
          ),
          channel.onClose((event) => {
            if (disposed) return;
            dispose();
            for (const handler of closeHandlers) handler(event);
          }),
        );
      }
    } catch (error) {
      dispose();
      throw error;
    }

    return {
      send(data) {
        if (disposed) throw new Error("Rust daemon connection is closed");
        if (typeof data !== "string") {
          if (!ready) throw new Error("Rust daemon is not ready");
          const bytes = data instanceof ArrayBuffer ? new Uint8Array(data) : data;
          if (!bytes.length) throw new Error("Empty Rust binary frame");
          // Rust preserves Paseo's binary opcode families: terminal < 0x10, files >= 0x10.
          channels[single ? 0 : bytes[0] < 0x10 ? 1 : 2].send(data);
          return;
        }
        const message = object(JSON.parse(data));
        if (message.type === "hello") {
          if (helloSent || opened.size !== capabilityGroups.length)
            throw new Error("Unexpected client hello");
          if (typeof message.clientId !== "string") throw new Error("Client ID is required");
          helloSent = true;
          for (const [index, channel] of channels.entries()) {
            channel.send(
              JSON.stringify({
                type: "hello",
                client_id: message.clientId,
                protocol: {
                  major: 1,
                  min_minor: 0,
                  max_minor: 0,
                },
                capabilities: capabilityGroups[index],
                required_capabilities: single ? ["connection.single.v1"] : [],
              }),
            );
          }
          return;
        }
        if (!ready) throw new Error("Rust daemon is not ready");
        if (message.type === "ping") request({ type: "ping" }, true);
        else if (message.type === "session") request(object(message.message));
        else throw new Error(`Unsupported client envelope: ${String(message.type)}`);
      },
      close: dispose,
      onOpen: (handler) => {
        openHandlers.add(handler);
        return () => {
          openHandlers.delete(handler);
        };
      },
      onClose: (handler) => {
        closeHandlers.add(handler);
        return () => {
          closeHandlers.delete(handler);
        };
      },
      onError: (handler) => {
        errorHandlers.add(handler);
        return () => {
          errorHandlers.delete(handler);
        };
      },
      onMessage: (handler) => {
        messageHandlers.add(handler);
        return () => {
          messageHandlers.delete(handler);
        };
      },
    };
  };
}

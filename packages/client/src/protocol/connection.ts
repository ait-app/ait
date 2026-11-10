import { UploadAcknowledgements } from "./upload-acks.js";
import { parseDaemonTransportUrl } from "./transport-url.js";
import { ClientMessageChunks } from "./chunks.js";
import { METHODS, type MethodSpec } from "./methods.js";
import { sessionEventKind } from "@ait/protocol/session-event-kinds";
import { eventMessage, responseMessage, rpcError, serverInfo } from "./messages.js";
import type { HubShareResourceKind } from "../hub/index.js";
import type { DaemonClientTrace } from "../daemon-client.js";
import { object, strings, type Payload, type Transport, type TransportFactory } from "./types.js";
import {
  asUint8Array,
  decodeFileTransferFrame,
  FileTransferOpcode,
} from "@ait/protocol/binary-frames/index";

const RUNTIME_METHODS: Readonly<Record<string, MethodSpec>> = {
  ...METHODS,
  "checkout.reset_workspace.request": {
    method: "checkout.reset_workspace.request",
    kind: "request",
    channel: 2,
    response: "checkout.reset_workspace.response",
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
  timer?: ReturnType<typeof setTimeout>;
}

/** SDK-owned Rust v1 session. Platform transports carry only wire frames. */
export interface MessageMetrics {
  bytes: number;
  decodeMs: number;
}

type MessageHandler = (data: unknown, binary: boolean, metrics?: MessageMetrics) => void;

export interface DaemonConnection extends Omit<Transport, "send" | "onMessage"> {
  onMessage(handler: MessageHandler): () => void;
  send(data: Payload | Uint8Array | ArrayBuffer, options?: { tracked?: boolean }): void;
  forget(requestId: string): void;
}

/** Hello capability that selects the daemon's single-socket mode (`connection.single.v1`). */
const SINGLE_CONNECTION = "connection.single.v1";
/** Server feature of daemons that accept `SINGLE_CONNECTION`. */
const SINGLE_FEATURE = "ait-rust-single-v1";
const ALL_CAPABILITIES = [...new Set(CHANNEL_CAPABILITIES.flat())];
const MAX_SPLIT_ENDPOINTS = 64;
/** Endpoints whose daemon rejected the single-connection hello; they reconnect with four sockets. */
const splitEndpoints = new Set<string>();

function rememberSplit(url: string): void {
  splitEndpoints.delete(url);
  splitEndpoints.add(url);
  if (splitEndpoints.size > MAX_SPLIT_ENDPOINTS)
    splitEndpoints.delete(splitEndpoints.values().next().value!);
}

/**
 * - `auto`: one socket with `connection.single.v1`, falling back to four sockets for daemons
 *   that reject it. Account relay sessions are always single.
 * - `split`: the four-socket mode only.
 */
export type DaemonConnectionMode = "auto" | "split";

export function createDaemonConnection(
  baseFactory: TransportFactory,
  {
    url,
    headers,
    guestScope,
    trace,
    mode = "auto",
  }: {
    url: string;
    headers?: Record<string, string>;
    guestScope?: HubShareResourceKind;
    trace?: DaemonClientTrace;
    mode?: DaemonConnectionMode;
  },
): DaemonConnection {
  const parsed = parseDaemonTransportUrl(url);
  const relay = parsed.protocol === "ait+desktop:" && parsed.hostname === "account-relay";
  // Direct, SSH and local daemons try one socket first; a rejection falls back to four
  // sockets once and is remembered for this endpoint. A Hub session is always single.
  let trial = !relay && mode === "auto" && !splitEndpoints.has(url);
  let single = relay || trial;
  let capabilityGroups = single ? [ALL_CAPABILITIES] : CHANNEL_CAPABILITIES;
  // SSH uses the desktop IPC URL; the main process validates its target and
  // opens only the Rust /v1/ws endpoint inside the authenticated tunnel.
  const ssh =
    parsed.protocol === "ait+desktop:" &&
    parsed.hostname === "ssh" &&
    (parsed.pathname === "" || parsed.pathname === "/");
  if (
    (!ssh && !relay && !/^(ws|wss):$/.test(parsed.protocol)) ||
    (!ssh && !relay && parsed.pathname !== "/v1/ws") ||
    parsed.username ||
    parsed.password ||
    (!ssh && !relay && parsed.search) ||
    parsed.hash
  ) {
    throw new Error("Invalid Rust daemon WebSocket endpoint");
  }
  const openHandlers = new Set<() => void>();
  const closeHandlers = new Set<(event?: unknown) => void>();
  const errorHandlers = new Set<(event?: unknown) => void>();
  const messageHandlers = new Set<MessageHandler>();
  const channels: Transport[] = [];
  const uploads = new UploadAcknowledgements(fail);
  const chunks = new ClientMessageChunks(channels, fail);
  /** Handlers of the current physical sockets; replaced when single mode falls back. */
  let channelCleanup: (() => void)[] = [];
  const opened = new Set<number>();
  const negotiated = new Map<number, Set<string>>();
  const subscriptions = new Map<string, number>();
  const pending = new Map<string, Pending>();
  let disposed = false;
  let ready = false;
  let helloSent = false;
  let clientId = "";
  let info: Payload | null = null;
  let implemented = new Set<string>();
  let setupTimer = setTimeout(
    () => fail(new Error("Rust daemon handshake timed out")),
    relay ? 45_000 : 10_000,
  );

  function traced<T>(name: string, args: Record<string, string>, run: () => T): T {
    let enabled = false;
    try {
      if (trace?.isEnabled()) {
        trace.beginSection(name, args);
        enabled = true;
      }
    } catch {
      /* Diagnostics must not interrupt a connection. */
    }
    try {
      return run();
    } finally {
      if (enabled) {
        try {
          trace?.endSection();
        } catch {
          /* Best-effort diagnostics. */
        }
      }
    }
  }

  function emit(value: Payload, metrics?: MessageMetrics): void {
    if (!disposed) for (const handler of messageHandlers) handler(value, false, metrics);
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
    chunks.dispose();
    uploads.dispose();
    subscriptions.clear();
    for (const remove of channelCleanup) remove();
    for (const channel of channels) {
      try {
        channel.close(code, reason);
      } catch {
        /* Already closed. */
      }
    }
  }

  /**
   * The daemon did not complete the single-connection hello: reopen with four sockets and
   * resend the caller's hello there. The caller sees one open and one server_info.
   */
  function fallBack(rejected: boolean): void {
    trial = false;
    single = false;
    capabilityGroups = CHANNEL_CAPABILITIES;
    // An explicit rejection identifies an older daemon; a bare close may be transient.
    if (rejected) rememberSplit(url);
    for (const remove of channelCleanup) remove();
    channelCleanup = [];
    for (const channel of channels.splice(0)) {
      try {
        channel.close(1000, "Single connection unsupported");
      } catch {
        /* Already closed. */
      }
    }
    opened.clear();
    negotiated.clear();
    info = null;
    clearTimeout(setupTimer);
    setupTimer = setTimeout(() => fail(new Error("Rust daemon handshake timed out")), 10_000);
    openChannels();
  }

  /** Before the first server_info, an unanswered single hello may mean an older daemon. */
  function canFallBack(): boolean {
    return trial && helloSent && !ready && !disposed && negotiated.size === 0;
  }

  function sendHello(): void {
    for (const [index, channel] of channels.entries()) {
      channel.send(
        JSON.stringify({
          type: "hello",
          client_id: clientId,
          protocol: {
            major: 1,
            min_minor: 0,
            max_minor: 0,
          },
          capabilities: capabilityGroups[index],
          required_capabilities: single ? [SINGLE_CONNECTION] : [],
        }),
      );
    }
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
    const wire = String(data);
    const now = () => (typeof performance !== "undefined" ? performance.now() : Date.now());
    const started = now();
    const message = traced("paseo.ws.json.parse", { size: String(wire.length) }, () =>
      object(JSON.parse(wire)),
    );
    const metrics = {
      bytes: new TextEncoder().encode(wire).length,
      decodeMs: now() - started,
    };
    const publish = (value: Payload) => emit(value, metrics);
    if (message.type === "server_info") {
      if (!helloSent || negotiated.has(channel)) throw new Error("Unexpected Rust daemon hello");
      const nextInfo = object(message.info);
      const protocol = object(nextInfo.protocol);
      if (protocol.major !== 1 || protocol.minor !== 0 || typeof nextInfo.server_id !== "string") {
        throw new Error("Unsupported Rust daemon protocol");
      }
      if (
        info &&
        (info.server_id !== nextInfo.server_id || info.instance_id !== nextInfo.instance_id)
      ) {
        throw new Error("Rust daemon restarted during handshake");
      }
      info = nextInfo;
      // An upgraded daemon behind a remembered endpoint gets single mode on the next connect.
      if (!single && Array.isArray(info.features) && info.features.includes(SINGLE_FEATURE))
        splitEndpoints.delete(url);
      implemented = new Set(strings(nextInfo.implemented_capabilities));
      negotiated.set(channel, new Set(strings(message.negotiated_capabilities)));
      if (negotiated.size === capabilityGroups.length) {
        ready = true;
        clearTimeout(setupTimer);
        publish(serverInfo(info, implemented, guestScope));
      }
      return;
    }
    if (!ready) {
      // Older daemons reject the required capability (or the capability count) of a single hello.
      if (message.type === "error" && canFallBack()) {
        fallBack(message.code === "unsupported_capability" || message.code === "invalid_message");
        return;
      }
      throw new Error(`Rust handshake rejected: ${String(message.code ?? message.type)}`);
    }
    if (message.type === "event" && message.method === "connection.upload.ack") {
      const ack = object(message.params);
      uploads.acknowledge(ack.requestId, ack.opcode);
      return;
    }
    if (message.type === "event" && message.method === "connection.chunk.ack") {
      const ack = object(message.params);
      chunks.acknowledge(channel, ack.id, ack.offset);
      return;
    }
    if (message.type === "event") {
      if (message.method === "status.server_info") {
        const params = object(message.params);
        const update = serverInfo(object(params.info), implemented, guestScope);
        const payload = object(update.payload);
        if (typeof params.subscriptionId === "string")
          payload.subscriptionId = params.subscriptionId;
        publish(update);
        return;
      }
      publish(eventMessage(String(message.method), message.params));
      return;
    }
    if (message.type !== "response" && message.type !== "error") {
      throw new Error("Unexpected Rust daemon envelope");
    }
    const id = message.request_id;
    if (typeof id !== "string") {
      // An event rejection (for example a terminal resize) is an application error.
      // Transport onError makes the SDK reconnect every channel and replay the failing event.
      publish(
        rpcError(
          crypto.randomUUID(),
          channel === 1 ? "terminal.input" : "event",
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
            sendWire(channel, item.wire);
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
      publish(
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
    if (item.rawPing) publish({ type: "connection.pong" });
    else if (item.spec.response)
      publish(responseMessage(item.spec.response, id, result, item.request));
  }

  function sendWire(channel: number, wire: string): void {
    chunks.send(
      channel,
      wire,
      Array.isArray(info?.features) && info.features.includes("client-message-chunks-v1"),
    );
  }

  function request(message: Payload, rawPing = false, tracked = false): void {
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
      : spec.method === "subscription.release.request" && typeof message.subscriptionId === "string"
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
    if (name === "connection.ping" && !rawPing) {
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
    if (name === "agent.permission.resolve.request") params.requestId = message.requestId;
    if (name === "session.events.set_subscription.request" && Array.isArray(params.events)) {
      params.events = strings(params.events).map(sessionEventKind);
    }
    if (rawPing) params.nonce = id;
    if (spec.kind !== "request") {
      sendWire(
        channel,
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
    const timer = tracked
      ? undefined
      : setTimeout(
          () => {
            const timedOut = pending.get(id);
            if (!pending.delete(id) || disposed) return;
            clearTimeout(timedOut?.retryTimer);
            emit(rpcError(id, name, "timeout", "Rust daemon request timed out"));
          },
          // Release the permission ID when its UI deadline expires so a user can retry.
          spec.method === "agent.permission.resolve.request"
            ? 5_000
            : spec.method === "file.upload.request"
              ? 30 * 60_000
              : 300_000,
        );
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
      sendWire(channel, wire);
    } catch (error) {
      pending.delete(id);
      clearTimeout(timer);
      throw error;
    }
  }

  function openChannels(): void {
    for (let index = 0; index < capabilityGroups.length; index += 1) {
      // Electron/native uses Bearer headers; browsers exchange them for one-use tickets.
      const raw = baseFactory({ url, headers });
      const channel: Transport = {
        send: (data) =>
          traced(
            "paseo.ws.frame.outbound",
            {
              kind: typeof data === "string" ? "text" : "binary",
              size: String(typeof data === "string" ? data.length : data.byteLength),
            },
            () => raw.send(data),
          ),
        close: (code, reason) => raw.close(code, reason),
        onOpen: (handler) => raw.onOpen(handler),
        onClose: (handler) => raw.onClose(handler),
        onError: (handler) => raw.onError(handler),
        onMessage: (handler) => raw.onMessage(handler),
        ...(raw.recoverLiveness ? { recoverLiveness: () => raw.recoverLiveness!() } : {}),
      };
      channels.push(channel);
      channelCleanup.push(
        channel.onOpen(() => {
          opened.add(index);
          if (opened.size !== capabilityGroups.length || disposed) return;
          // After a fallback the caller's hello was already accepted; repeat it on the new sockets.
          if (helloSent) {
            try {
              sendHello();
            } catch (error) {
              fail(error instanceof Error ? error : new Error("Rust daemon hello failed"));
            }
          } else for (const handler of openHandlers) handler();
        }),
        channel.onMessage((data, binary) => {
          try {
            traced(
              "paseo.ws.frame.inbound",
              {
                kind: binary ? "binary" : "text",
                size: String(binary ? (asUint8Array(data)?.length ?? 0) : String(data).length),
              },
              () => receive(index, data, binary),
            );
          } catch (error) {
            fail(error instanceof Error ? error : new Error("Invalid Rust daemon response"));
          }
        }),
        channel.onError((error) => {
          if (canFallBack()) {
            fallBack(false);
            return;
          }
          fail(
            error instanceof Error
              ? error
              : new Error("Rust daemon transport failed; check address and Bearer token"),
          );
        }),
        channel.onClose((event) => {
          if (disposed) return;
          if (canFallBack()) {
            fallBack(false);
            return;
          }
          dispose();
          for (const handler of closeHandlers) handler(event);
        }),
      );
    }
  }

  try {
    openChannels();
  } catch (error) {
    dispose();
    throw error;
  }

  return {
    send(data, options) {
      if (disposed) throw new Error("Rust daemon connection is closed");
      if (data instanceof Uint8Array || data instanceof ArrayBuffer) {
        if (!ready) throw new Error("Rust daemon is not ready");
        const bytes = data instanceof ArrayBuffer ? new Uint8Array(data) : data;
        if (!bytes.length) throw new Error("Empty Rust binary frame");
        // Rust preserves Paseo's binary opcode families: terminal < 0x10, files >= 0x10.
        const channel = channels[single ? 0 : bytes[0] < 0x10 ? 1 : 2];
        if (
          bytes[0] >= 0x10 &&
          bytes[0] <= 0x12 &&
          Array.isArray(info?.features) &&
          info.features.includes("client-message-chunks-v1")
        )
          uploads.send(channel, bytes);
        else channel.send(data);
        return;
      }
      const message = data;
      if (message.type === "hello") {
        if (helloSent || opened.size !== capabilityGroups.length)
          throw new Error("Unexpected client hello");
        if (typeof message.clientId !== "string") throw new Error("Client ID is required");
        helloSent = true;
        clientId = message.clientId;
        sendHello();
        return;
      }
      if (!ready) throw new Error("Rust daemon is not ready");
      if (message.type === "connection.ping") request({ type: "connection.ping" }, true);
      else request(message, false, options?.tracked);
    },
    forget(requestId) {
      const item = pending.get(requestId);
      if (item) finishPending(requestId, item);
    },
    close: dispose,
    // Business pings travel on channel zero, including the single Hub session.
    recoverLiveness: () =>
      !disposed && channels[0]?.recoverLiveness
        ? channels[0].recoverLiveness()
        : Promise.resolve(false),
    drain: () => uploads.drain(),
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
}

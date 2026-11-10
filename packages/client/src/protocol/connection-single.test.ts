import { afterEach, describe, expect, it, vi } from "vitest";
import {
  CHANNEL_CAPABILITIES,
  createDaemonConnection,
  type DaemonConnectionMode,
} from "./connection.js";
import { METHODS } from "./methods.js";
import { object, type Payload, type TransportFactory } from "./types.js";

const ALL = [...new Set(CHANNEL_CAPABILITIES.flat())];
const IMPLEMENTED = Object.values(METHODS).map((spec) => spec.method);

interface FakeSocket {
  send: ReturnType<typeof vi.fn>;
  close: ReturnType<typeof vi.fn>;
  open(): void;
  message(value: unknown): void;
  end(): void;
  fail(): void;
  hello(): Payload;
}

function connect(url: string, mode?: DaemonConnectionMode) {
  const sockets: FakeSocket[] = [];
  const factory: TransportFactory = () => {
    let onOpen = () => {};
    let onMessage = (_value: unknown, _binary: boolean) => {};
    let onError = (_event?: unknown) => {};
    let onClose = (_event?: unknown) => {};
    const socket: FakeSocket = {
      send: vi.fn(),
      close: vi.fn(),
      open: () => onOpen(),
      message: (value) => onMessage(JSON.stringify(value), false),
      end: () => onClose({ code: 1006 }),
      fail: () => onError(new Error("socket reset")),
      hello: () => object(JSON.parse(socket.send.mock.calls[0][0])),
    };
    sockets.push(socket);
    return {
      send: socket.send,
      close: socket.close,
      onOpen: (fn) => ((onOpen = fn), () => (onOpen = () => {})),
      onMessage: (fn) => ((onMessage = fn), () => (onMessage = () => {})),
      onError: (fn) => ((onError = fn), () => (onError = () => {})),
      onClose: (fn) => ((onClose = fn), () => (onClose = () => {})),
    };
  };
  const base = vi.fn(factory);
  const transport = createDaemonConnection(base, {
    url,
    headers: { Authorization: "Bearer test" },
    ...(mode ? { mode } : {}),
  });
  const received: Payload[] = [];
  const opened = vi.fn(() => transport.send({ type: "hello", clientId: "single-test" }));
  const errors = vi.fn();
  const closed = vi.fn();
  transport.onOpen(opened);
  transport.onMessage((value, binary) => {
    if (!binary) received.push(object(value));
  });
  transport.onError(errors);
  transport.onClose(closed);
  function ready(features: string[] = ["ait-rust-single-v1"]) {
    for (const socket of sockets) {
      if (socket.close.mock.calls.length) continue;
      socket.open();
    }
    const live = sockets.filter((socket) => !socket.close.mock.calls.length);
    for (const [index, socket] of live.entries())
      socket.message({
        type: "server_info",
        info: {
          server_id: "server",
          instance_id: "instance",
          features,
          protocol: { major: 1, minor: 0 },
          implemented_capabilities: IMPLEMENTED,
        },
        negotiated_capabilities: live.length === 1 ? ALL : CHANNEL_CAPABILITIES[index],
      });
  }
  return { transport, sockets, base, received, opened, errors, closed, ready };
}

let port = 9000;
/** Each test uses its own endpoint: the fallback memory is per endpoint URL. */
function endpoint(): string {
  port += 1;
  return `ws://127.0.0.1:${port}/v1/ws`;
}

describe("single-connection daemon transport", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it.each([
    ["direct", "ws://127.0.0.1:7400/v1/ws"],
    ["SSH", "ait+desktop://ssh?host=build-box&daemonPort=7316"],
  ])("carries a %s daemon on one socket with every capability", (_kind, url) => {
    const h = connect(url);
    try {
      expect(h.base).toHaveBeenCalledOnce();
      h.ready();
      expect(h.sockets[0].hello()).toEqual({
        type: "hello",
        client_id: "single-test",
        protocol: { major: 1, min_minor: 0, max_minor: 0 },
        capabilities: ALL,
        required_capabilities: ["connection.single.v1"],
      });
      expect(h.received).toEqual([expect.objectContaining({ type: "status" })]);
      // Every channel's requests and binary frames share the socket.
      h.transport.send({ type: "project.list.request", requestId: "projects" });
      h.transport.send({ type: "publication.status.request", requestId: "agents" });
      h.transport.send(new Uint8Array([1, 2, 3]));
      expect(h.sockets[0].send.mock.calls.slice(1).map(([data]) => data)).toEqual([
        expect.stringContaining('"method":"project.list.request"'),
        expect.stringContaining('"request_id":"agents"'),
        new Uint8Array([1, 2, 3]),
      ]);
      h.sockets[0].message({ type: "response", request_id: "projects", result: { projects: [] } });
      expect(h.received.at(-1)).toMatchObject({ type: "project.list.response" });
      expect(h.errors).not.toHaveBeenCalled();
    } finally {
      h.transport.close();
    }
  });

  it("falls back to four sockets when an older daemon rejects the single hello", () => {
    const url = endpoint();
    const h = connect(url);
    try {
      h.sockets[0].open();
      expect(h.opened).toHaveBeenCalledOnce();
      h.sockets[0].message({
        type: "error",
        request_id: null,
        code: "unsupported_capability",
        message: "Unsupported capability",
        retryable: false,
      });
      expect(h.sockets[0].close).toHaveBeenCalled();
      expect(h.base).toHaveBeenCalledTimes(5);
      h.ready([]);
      // The caller's hello is repeated on each new socket; it does not see a second open.
      expect(h.opened).toHaveBeenCalledOnce();
      for (const [index, socket] of h.sockets.slice(1).entries())
        expect(socket.hello()).toEqual({
          type: "hello",
          client_id: "single-test",
          protocol: { major: 1, min_minor: 0, max_minor: 0 },
          capabilities: CHANNEL_CAPABILITIES[index],
          required_capabilities: [],
        });
      expect(h.received).toHaveLength(1);
      expect(h.received[0]).toMatchObject({ type: "status" });
      expect(h.errors).not.toHaveBeenCalled();
      expect(h.closed).not.toHaveBeenCalled();
      h.transport.send({ type: "project.list.request", requestId: "projects" });
      expect(h.sockets[2].send.mock.lastCall![0]).toContain("project.list.request");
    } finally {
      h.transport.close();
    }
    // The rejection is remembered: reconnects go straight to four sockets.
    const next = connect(url);
    try {
      expect(next.base).toHaveBeenCalledTimes(4);
      next.ready([]);
      expect(next.sockets[0].hello()).toMatchObject({ required_capabilities: [] });
    } finally {
      next.transport.close();
    }
  });

  it("falls back when an older daemon rejects the capability count", () => {
    const h = connect(endpoint());
    try {
      h.sockets[0].open();
      h.sockets[0].message({ type: "error", code: "invalid_message", message: "Invalid" });
      expect(h.base).toHaveBeenCalledTimes(5);
      h.ready([]);
      expect(h.received).toHaveLength(1);
      expect(h.errors).not.toHaveBeenCalled();
    } finally {
      h.transport.close();
    }
  });

  it("falls back for one attempt when the socket closes before server_info", () => {
    const url = endpoint();
    const h = connect(url);
    try {
      h.sockets[0].open();
      h.sockets[0].end();
      expect(h.base).toHaveBeenCalledTimes(5);
      h.ready([]);
      expect(h.received).toHaveLength(1);
      expect(h.closed).not.toHaveBeenCalled();
    } finally {
      h.transport.close();
    }
    // A bare close may be transient, so the next connection tries single mode again.
    const next = connect(url);
    try {
      expect(next.base).toHaveBeenCalledOnce();
    } finally {
      next.transport.close();
    }
  });

  it("falls back when the socket errors before server_info", () => {
    const h = connect(endpoint());
    try {
      h.sockets[0].open();
      h.sockets[0].fail();
      h.sockets[0].end();
      expect(h.base).toHaveBeenCalledTimes(5);
      h.ready([]);
      expect(h.received).toHaveLength(1);
      expect(h.errors).not.toHaveBeenCalled();
      expect(h.closed).not.toHaveBeenCalled();
    } finally {
      h.transport.close();
    }
  });

  it("forgets a remembered fallback once the daemon advertises single mode", () => {
    const url = endpoint();
    const first = connect(url);
    first.sockets[0].open();
    first.sockets[0].message({ type: "error", code: "unsupported_capability", message: "No" });
    first.transport.close();
    const upgraded = connect(url);
    try {
      expect(upgraded.base).toHaveBeenCalledTimes(4);
      upgraded.ready(["ait-rust-single-v1"]);
    } finally {
      upgraded.transport.close();
    }
    const next = connect(url);
    try {
      expect(next.base).toHaveBeenCalledOnce();
    } finally {
      next.transport.close();
    }
  });

  it("fails normally when the daemon is unreachable or the four-socket retry fails", () => {
    const unreachable = connect(endpoint());
    unreachable.sockets[0].end();
    expect(unreachable.base).toHaveBeenCalledOnce();
    expect(unreachable.closed).toHaveBeenCalledOnce();

    const h = connect(endpoint());
    h.sockets[0].open();
    h.sockets[0].message({ type: "error", code: "unsupported_capability", message: "No" });
    for (const socket of h.sockets.slice(1)) socket.open();
    h.sockets[1].message({ type: "error", code: "unsupported_capability", message: "No" });
    expect(h.base).toHaveBeenCalledTimes(5);
    expect(h.errors).toHaveBeenCalledWith(
      expect.objectContaining({ message: "Rust handshake rejected: unsupported_capability" }),
    );
  });

  it("gives the four-socket fallback its own handshake budget", () => {
    vi.useFakeTimers();
    const h = connect(endpoint());
    vi.advanceTimersByTime(9_000);
    h.sockets[0].open();
    h.sockets[0].message({ type: "error", code: "unsupported_capability", message: "No" });
    vi.advanceTimersByTime(9_000);
    expect(h.errors).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1_000);
    expect(h.errors).toHaveBeenCalledWith(
      expect.objectContaining({ message: "Rust daemon handshake timed out" }),
    );
  });

  it("keeps account relay sessions single without a fallback", () => {
    const h = connect("ait+desktop://account-relay/host?center=https://center.test");
    h.sockets[0].open();
    expect(h.sockets[0].hello()).toMatchObject({
      capabilities: ALL,
      required_capabilities: ["connection.single.v1"],
    });
    h.sockets[0].message({ type: "error", code: "unsupported_capability", message: "No" });
    expect(h.base).toHaveBeenCalledOnce();
    expect(h.errors).toHaveBeenCalledOnce();
    expect(h.closed).toHaveBeenCalledOnce();
  });

  it("uses four sockets when the caller asks for the split mode", () => {
    const h = connect(endpoint(), "split");
    try {
      expect(h.base).toHaveBeenCalledTimes(4);
      h.ready([]);
      expect(h.sockets[3].hello()).toMatchObject({
        capabilities: CHANNEL_CAPABILITIES[3],
        required_capabilities: [],
      });
    } finally {
      h.transport.close();
    }
  });

  it("closes every split socket even when one close throws on an illegal code", () => {
    const h = connect(endpoint(), "split");
    const browserClose = (code?: number) => {
      if (code !== undefined && code !== 1000 && (code < 3000 || code > 4999))
        throw new DOMException("Illegal close code", "InvalidAccessError");
    };
    for (const socket of h.sockets) socket.close.mockImplementation(browserClose);

    expect(() => h.transport.close(1001, "Reconnecting")).not.toThrow();

    for (const socket of h.sockets) expect(socket.close).toHaveBeenCalledOnce();
  });
});

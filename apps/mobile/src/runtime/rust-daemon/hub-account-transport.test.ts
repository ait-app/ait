import { DaemonClient } from "@ait/client/internal/daemon-client";
import { HubLink } from "@ait/client/internal/hub";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createHubAccountRelayTransportFactory } from "./hub-account-transport";
import { createRustDaemonTransportFactory } from "./transport";

const HOST_ID = "11111111-1111-4111-8111-111111111111";
const CENTER = "https://example.test/api";
const url = `ait+desktop://account-relay/${HOST_ID}?center=${encodeURIComponent(CENTER)}`;
const SERVER_INFO = {
  type: "server_info",
  info: {
    server_id: "server",
    instance_id: "instance",
    features: ["ait-rust-single-v1"],
    protocol: { major: 1, minor: 0 },
    implemented_capabilities: ["connection.ping"],
  },
  negotiated_capabilities: ["connection.ping"],
};

/** Minimal center side of one hub WebSocket. */
class CenterSocket {
  readyState = 0;
  binaryType = "blob";
  readonly sent: { header: Record<string, unknown>; payload: string }[] = [];
  readonly close = vi.fn();
  private readonly listeners = new Map<string, (event: unknown) => void>();

  addEventListener(event: string, listener: (event: unknown) => void) {
    this.listeners.set(event, listener);
  }
  removeEventListener(event: string) {
    this.listeners.delete(event);
  }
  send(raw: string | Uint8Array | ArrayBuffer) {
    if (typeof raw === "string") throw new Error("Hub frames must be binary");
    const data = raw instanceof Uint8Array ? raw : new Uint8Array(raw);
    const length = (data[0] << 8) | data[1];
    const decoder = new TextDecoder();
    this.sent.push({
      header: JSON.parse(decoder.decode(data.subarray(2, 2 + length))),
      payload: decoder.decode(data.subarray(2 + length)),
    });
  }
  open() {
    this.readyState = 1;
    this.listeners.get("open")?.({});
  }
  receive(header: object, payload = "") {
    const encoder = new TextEncoder();
    const json = encoder.encode(JSON.stringify(header));
    const body = encoder.encode(payload);
    const frame = new Uint8Array(2 + json.byteLength + body.byteLength);
    frame.set([json.byteLength >> 8, json.byteLength & 0xff]);
    frame.set(json, 2);
    frame.set(body, 2 + json.byteLength);
    this.listeners.get("message")?.({ data: frame.buffer });
  }
  drop() {
    this.listeners.get("close")?.({ code: 1006 });
  }
}

function harness(nodes: object[] = [{ node_id: "host-node", host_id: HOST_ID, state: "online" }]) {
  vi.useFakeTimers();
  const sockets: CenterSocket[] = [];
  const link = new HubLink({
    ticket: async () => ({ centerUrl: CENTER, ticket: "a".repeat(64) }),
    webSocketFactory: () => {
      const socket = new CenterSocket();
      sockets.push(socket);
      return socket;
    },
    randomUUID: () => "session-1",
    logger: { info: () => {}, warn: () => {} },
  });
  const closes = new Set<() => void>();
  const deps = {
    hub: vi.fn((): HubLink | Promise<HubLink | null> | null => link),
    register: vi.fn((close: () => void) => {
      closes.add(close);
      return () => {
        closes.delete(close);
      };
    }),
  };
  const connect = async () => {
    link.start();
    await vi.advanceTimersByTimeAsync(0);
    const socket = sockets.at(-1)!;
    socket.open();
    socket.receive({
      t: "welcome",
      v: 1,
      link: "link-1",
      resumed: false,
      recv: 0,
      resume_token: "r".repeat(64),
      node_id: "phone",
      grace_seconds: 120,
      ping_seconds: 20,
      max_payload: 1024 * 1024,
    });
    socket.receive({ t: "presence", seq: 1 }, JSON.stringify({ nodes }));
    return socket;
  };
  return { link, sockets, deps, closes, connect };
}

afterEach(() => vi.useRealTimers());

describe("native account relay path selection", () => {
  it("runs the unchanged Rust client over a hub session and survives a link reconnect", async () => {
    const h = harness();
    const socket = await h.connect();
    const transport = createRustDaemonTransportFactory(
      createHubAccountRelayTransportFactory(h.deps),
    )({ url });
    const receive = vi.fn();
    const close = vi.fn();
    transport.onMessage(receive);
    transport.onClose(close);
    transport.onOpen(() => transport.send(JSON.stringify({ type: "hello", clientId: "android" })));
    await vi.advanceTimersByTimeAsync(0);
    const open = socket.sent.find((frame) => frame.header.t === "open")!;
    expect(open.header).toEqual({ t: "open", seq: 1, s: "session-1", to: { node: "host-node" } });
    expect(JSON.parse(open.payload)).toMatchObject({
      type: "hello",
      client_id: "android",
      required_capabilities: ["connection.single.v1"],
    });
    socket.receive({ t: "accept", seq: 2, s: "session-1" }, JSON.stringify(SERVER_INFO));
    expect(receive.mock.calls.at(-1)?.[0]).toContain("server_info");

    socket.drop();
    await vi.advanceTimersByTimeAsync(0);
    const resumed = h.sockets.at(-1)!;
    resumed.open();
    resumed.receive({
      t: "welcome",
      v: 1,
      link: "link-1",
      resumed: true,
      recv: 1,
      node_id: "phone",
      grace_seconds: 120,
      ping_seconds: 20,
      max_payload: 1024 * 1024,
    });
    transport.send('{"type":"connection.ping"}');
    const ping = JSON.parse(resumed.sent.at(-1)!.payload);
    expect(ping).toMatchObject({ type: "request", method: "connection.ping" });
    resumed.receive(
      { t: "msg", seq: 3, s: "session-1", pt: "text" },
      JSON.stringify({ type: "response", request_id: ping.request_id, result: {} }),
    );
    expect(receive).toHaveBeenLastCalledWith('{"type":"connection.pong"}', false);
    expect(close).not.toHaveBeenCalled();

    // Sign-out and suspension close hub sessions with the other account transports.
    expect(h.closes.size).toBe(1);
    for (const end of h.closes) end();
    expect(close).toHaveBeenCalledOnce();
    expect(resumed.sent.at(-1)!.header).toEqual({
      t: "close",
      seq: 3,
      s: "session-1",
      code: "closed",
    });
  });

  it("reports a Host without a serve endpoint as unavailable", async () => {
    const h = harness([{ node_id: "other", host_id: "other-host", state: "online" }]);
    await h.connect();
    const transport = createHubAccountRelayTransportFactory(h.deps)({ url });
    const error = vi.fn();
    const close = vi.fn();
    transport.onError(error);
    transport.onClose(close);
    await vi.advanceTimersByTimeAsync(0);
    expect(error).toHaveBeenCalledWith(new Error("Target Host is unavailable on Hub."));
    expect(close).toHaveBeenCalledOnce();
    expect(h.sockets[0].sent.some(({ header }) => header.t === "open")).toBe(false);
    h.link.stop();
  });

  it.each(["absent", "stopped", "other-center"])("reports an unavailable Hub: %s", async (mode) => {
    const h = harness();
    if (mode === "absent") h.deps.hub.mockReturnValue(null);
    if (mode === "other-center") await h.connect();
    const target =
      mode === "other-center"
        ? `ait+desktop://account-relay/${HOST_ID}?center=${encodeURIComponent("https://other.test/api")}`
        : url;
    const error = vi.fn();
    createHubAccountRelayTransportFactory(h.deps)({ url: target }).onError(error);
    await vi.advanceTimersByTimeAsync(0);
    expect(error).toHaveBeenCalledWith(new Error("Online Hub is unavailable."));
    h.link.stop();
  });

  it("rejects invalid targets before opening a session", () => {
    const h = harness();
    expect(() =>
      createHubAccountRelayTransportFactory(h.deps)({
        url: `ait+desktop://account-relay/${HOST_ID}`,
      }),
    ).toThrow("Invalid account relay target.");
    expect(h.deps.hub).not.toHaveBeenCalled();
  });

  it("waits for a connecting Hub and reports an error if the bounded wait expires", async () => {
    const h = harness();
    h.link.start();
    const pending = createHubAccountRelayTransportFactory(h.deps)({ url });
    const opened = vi.fn();
    pending.onOpen(opened);
    await vi.advanceTimersByTimeAsync(0);
    const socket = h.sockets.at(-1)!;
    socket.open();
    socket.receive({
      t: "welcome",
      v: 1,
      link: "link-1",
      resumed: false,
      recv: 0,
      resume_token: "r".repeat(64),
      node_id: "phone",
      grace_seconds: 120,
      ping_seconds: 20,
      max_payload: 1024 * 1024,
    });
    socket.receive(
      { t: "presence", seq: 1 },
      JSON.stringify({ nodes: [{ node_id: "host-node", host_id: HOST_ID, state: "online" }] }),
    );
    await vi.advanceTimersByTimeAsync(0);
    expect(opened).toHaveBeenCalledOnce();

    const slow = harness();
    slow.link.start();
    const pendingError = createHubAccountRelayTransportFactory(slow.deps)({ url });
    const error = vi.fn();
    pendingError.onError(error);
    await vi.advanceTimersByTimeAsync(4999);
    expect(error).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(error).toHaveBeenCalledWith(new Error("Online Hub is unavailable."));
    pending.close();
    h.link.stop();
    slow.link.stop();
  });
});

describe("deferred account relay transport", () => {
  it("reports a failed account restore as an error and close", async () => {
    const h = harness();
    h.deps.hub.mockReturnValue(Promise.reject(new Error("restore failed")));
    const transport = createHubAccountRelayTransportFactory(h.deps)({ url });
    const error = vi.fn();
    const close = vi.fn();
    transport.onError(error);
    transport.onClose(close);
    expect(() => transport.send("early")).toThrow("Relay is not ready.");
    await vi.advanceTimersByTimeAsync(0);
    expect(error).toHaveBeenCalledWith(new Error("restore failed"));
    expect(close).toHaveBeenCalledWith({ code: 1006, reason: "restore failed" });
    transport.close();
  });

  it("never creates a transport once closed while the choice is pending", async () => {
    const h = harness();
    let resolve!: (link: HubLink | null) => void;
    h.deps.hub.mockReturnValue(new Promise((done) => (resolve = done)));
    const transport = createHubAccountRelayTransportFactory(h.deps)({ url });
    transport.close();
    resolve(null);
    await vi.advanceTimersByTimeAsync(0);
    expect(() => transport.send("late")).toThrow("Relay is not ready.");
  });

  it("forwards deferred Hub events only to subscribed handlers", async () => {
    const h = harness();
    const socket = await h.connect();
    h.deps.hub.mockReturnValue(Promise.resolve(h.link));
    const transport = createHubAccountRelayTransportFactory(h.deps)({ url });
    const kept = { open: vi.fn(), close: vi.fn(), error: vi.fn(), message: vi.fn() };
    const removed = { open: vi.fn(), close: vi.fn(), error: vi.fn(), message: vi.fn() };
    for (const events of [kept, removed]) {
      const removers = [
        transport.onOpen(events.open),
        transport.onClose(events.close),
        transport.onError(events.error),
        transport.onMessage(events.message),
      ];
      if (events === removed) for (const remove of removers) remove();
    }
    await vi.advanceTimersByTimeAsync(0);
    transport.send('{"type":"hello"}');
    socket.receive({ t: "accept", seq: 2, s: "session-1" }, JSON.stringify(SERVER_INFO));
    socket.receive({ t: "msg", seq: 3, s: "session-1", pt: "text" }, "data");
    expect(kept.open).toHaveBeenCalledOnce();
    expect(kept.message).toHaveBeenLastCalledWith("data", false);
    for (const handler of Object.values(removed)) expect(handler).not.toHaveBeenCalled();
    transport.close();
    expect(h.closes.size).toBe(0);
    h.link.stop();
  });

  it("reports a hub session the account cannot track as a failed transport", async () => {
    const h = harness();
    await h.connect();
    h.deps.register.mockImplementation(() => {
      throw new Error("Account is signing out.");
    });
    const transport = createHubAccountRelayTransportFactory(h.deps)({ url });
    const error = vi.fn();
    transport.onError(error);
    await vi.advanceTimersByTimeAsync(0);
    expect(error).toHaveBeenCalledWith(new Error("Account is signing out."));
    expect(h.sockets.at(-1)!.sent.filter((frame) => frame.header.t === "open")).toEqual([]);
  });
});

describe("Hub recovery with business liveness", () => {
  it.each([false, true])(
    "preserves the session on foreground recovery (deferred=%s)",
    async (defer) => {
      const h = harness();
      const socket = await h.connect();
      if (defer) h.deps.hub.mockReturnValue(Promise.resolve(h.link));
      const client = new DaemonClient({
        url,
        clientId: "foreground",
        reconnect: { enabled: false },
        transportFactory: createRustDaemonTransportFactory(
          createHubAccountRelayTransportFactory(h.deps),
        ),
      });
      try {
        const connected = client.connect();
        await vi.advanceTimersByTimeAsync(0);
        socket.receive({ t: "accept", seq: 2, s: "session-1" }, JSON.stringify(SERVER_INFO));
        await connected;
        // Both foreground listeners run against a silently lost physical connection.
        const verified = h.link.verify();
        client.ensureConnected({ verify: true });
        await vi.advanceTimersByTimeAsync(3001);
        await expect(verified).resolves.toBe(false);
        expect(h.link.state).toBe("reconnecting");
        expect(client.isConnected).toBe(true);
        expect(h.closes.size).toBe(1);
        // Leave recovery pending long enough to also cross a normal heartbeat deadline.
        await vi.advanceTimersByTimeAsync(30_000);
        const resumed = h.sockets.at(-1)!;
        resumed.open();
        resumed.receive({
          t: "welcome",
          v: 1,
          link: "link-1",
          resumed: true,
          recv: 1,
          node_id: "phone",
          grace_seconds: 120,
        });
        await vi.advanceTimersByTimeAsync(0);
        const ping = resumed.sent.filter((frame) => frame.header.t === "msg").at(-1)!;
        const request = JSON.parse(ping.payload);
        resumed.receive(
          { t: "msg", seq: 3, s: "session-1", pt: "text" },
          JSON.stringify({
            type: "response",
            request_id: request.request_id,
            result: { nonce: request.params.nonce },
          }),
        );
        await vi.advanceTimersByTimeAsync(3001);
        expect(client.isConnected).toBe(true);
        expect(h.closes.size).toBe(1);
        expect(socket.sent.filter((frame) => frame.header.t === "open")).toHaveLength(1);
        expect(
          resumed.sent.some((frame) => frame.header.t === "open" || frame.header.t === "close"),
        ).toBe(false);
      } finally {
        await client.close();
        h.link.stop();
      }
    },
  );

  it("still closes an unresponsive daemon when the hub itself answers", async () => {
    const h = harness();
    const socket = await h.connect();
    const client = new DaemonClient({
      url,
      clientId: "silent-daemon",
      reconnect: { enabled: false },
      transportFactory: createRustDaemonTransportFactory(
        createHubAccountRelayTransportFactory(h.deps),
      ),
    });
    try {
      const connected = client.connect();
      await vi.advanceTimersByTimeAsync(0);
      socket.receive({ t: "accept", seq: 2, s: "session-1" }, JSON.stringify(SERVER_INFO));
      await connected;
      client.ensureConnected({ verify: true });
      await vi.advanceTimersByTimeAsync(3000);
      const ping = socket.sent.find((frame) => frame.header.t === "ping")!;
      socket.receive({ t: "pong", n: ping.header.n });
      await vi.advanceTimersByTimeAsync(0);
      expect(socket.sent.some((frame) => frame.header.t === "close")).toBe(true);
      expect(h.link.state).toBe("online");
    } finally {
      await client.close();
      h.link.stop();
    }
  });
});

import { EventEmitter, once } from "node:events";
import { encodeTerminalStreamFrame, TerminalStreamOpcode } from "@ait/protocol/binary-frames/index";
import { WebSocketServer } from "ws";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AccountSessionManager } from "./account-session.js";
import { AccountTransportManager, type AccountTransportOwner } from "./account-transport.js";

const sessionId = "account-terminal-burst";
const cleanups: (() => Promise<void>)[] = [];

afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
  vi.useRealTimers();
});

function terminalFrames(count: number): Buffer[] {
  return Array.from({ length: count }, (_, index) =>
    Buffer.from(
      encodeTerminalStreamFrame({
        opcode: TerminalStreamOpcode.Output,
        slot: 1,
        payload: `terminal output ${index}\r\n`,
      }),
    ),
  );
}

async function openRelay(synchronousAck = false) {
  const server = new WebSocketServer({ host: "127.0.0.1", port: 0 });
  await once(server, "listening");
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("Missing test relay address");
  const account = {
    openVisit: vi.fn(async () => ({
      relay_session_id: "test-visit",
      server_id: "test-server",
      instance_id: "test-instance",
      client_ticket: "test-only-ticket",
      url: `ws://127.0.0.1:${address.port}`,
    })),
    closeVisit: vi.fn(async () => {}),
  };
  const manager = new AccountTransportManager(account as unknown as AccountSessionManager);
  const events: Record<string, unknown>[] = [];
  const scheduledAcks = new Set<NodeJS.Immediate>();
  const pendingAcks = new Set<number>();
  let automaticAck = true;
  let maxPendingAcks = 0;
  const acknowledge = (sequence: number) => {
    pendingAcks.delete(sequence);
    manager.acknowledge(owner, sessionId, sequence);
  };
  const owner: AccountTransportOwner & EventEmitter = Object.assign(new EventEmitter(), {
    isDestroyed: () => false,
    send: (_channel: string, payload: Record<string, unknown>) => {
      events.push(payload);
      if (payload.kind === "open") {
        void manager.send(owner, sessionId, { text: JSON.stringify({ type: "hello" }) });
      }
      if (payload.kind !== "message" || typeof payload.sequence !== "number") return;
      const sequence = payload.sequence;
      pendingAcks.add(sequence);
      maxPendingAcks = Math.max(maxPendingAcks, pendingAcks.size);
      if (!automaticAck) return;
      if (synchronousAck) {
        acknowledge(sequence);
        return;
      }
      const scheduled = setImmediate(() => {
        scheduledAcks.delete(scheduled);
        if (!events.some((event) => event.kind === "close")) acknowledge(sequence);
      });
      scheduledAcks.add(scheduled);
    },
  });
  let burst = (_frames: Buffer[]) => {
    throw new Error("Test relay is not connected");
  };
  server.on("connection", (socket, request) => {
    socket.send(JSON.stringify({ type: "relay.ready", relay_session_id: "test-visit" }));
    socket.once("message", () => {
      socket.send(
        JSON.stringify({
          type: "server_info",
          info: {
            server_id: "test-server",
            instance_id: "test-instance",
            features: ["ait-rust-single-v1"],
          },
        }),
      );
    });
    burst = (frames) => {
      // Real ws frames coalesced in one TCP read reproduce pause()'s buffered dispatch.
      request.socket.cork();
      for (const frame of frames) socket.send(frame);
      request.socket.uncork();
    };
  });
  cleanups.push(async () => {
    for (const scheduled of scheduledAcks) clearImmediate(scheduled);
    manager.closeAll();
    for (const client of server.clients) client.terminate();
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve())),
    );
  });
  await manager.open(
    owner,
    sessionId,
    "11111111-1111-4111-8111-111111111111",
    "https://test.invalid/api",
  );
  await vi.waitFor(() => {
    expect(events.some((event) => typeof event.text === "string")).toBe(true);
    expect(pendingAcks.size).toBe(0);
  });
  return {
    manager,
    owner,
    account,
    events,
    burst,
    acknowledge,
    pendingAcks,
    manualAck: () => {
      automaticAck = false;
    },
    maxPendingAcks: () => maxPendingAcks,
    frames: () => events.filter((event) => typeof event.binaryBase64 === "string"),
    closes: () => events.filter((event) => event.kind === "close"),
  };
}

describe("desktop relay terminal delivery", () => {
  it.each([16, 17, 32, 256])(
    "keeps the host connected and preserves all %i frames with asynchronous renderer ACKs",
    async (frameCount) => {
      const relay = await openRelay();
      const frames = terminalFrames(frameCount);
      relay.burst(frames);
      await vi.waitFor(() =>
        expect(relay.frames().length === frameCount || relay.closes().length > 0).toBe(true),
      );
      expect(relay.closes()).toEqual([]);
      expect(relay.frames().map((event) => event.binaryBase64)).toEqual(
        frames.map((frame) => frame.toString("base64")),
      );
      expect(relay.maxPendingAcks()).toBeLessThanOrEqual(16);
    },
  );

  it("supports synchronous consumers without reordering frames", async () => {
    const relay = await openRelay(true);
    const frames = terminalFrames(32);
    relay.burst(frames);
    await vi.waitFor(() => expect(relay.frames()).toHaveLength(frames.length));
    expect(relay.closes()).toEqual([]);
    expect(relay.frames().map((event) => event.binaryBase64)).toEqual(
      frames.map((frame) => frame.toString("base64")),
    );
  });

  it("delivers queued frames only as the renderer acknowledges its delivery window", async () => {
    const relay = await openRelay();
    relay.manualAck();
    const frames = terminalFrames(32);
    relay.burst(frames);
    await vi.waitFor(() => expect(relay.frames()).toHaveLength(16));
    expect(relay.closes()).toEqual([]);

    const firstSequence = relay.pendingAcks.values().next().value!;
    relay.acknowledge(firstSequence);
    expect(relay.frames()).toHaveLength(17);
    // Duplicate ACKs must not grant another delivery slot.
    relay.acknowledge(firstSequence);
    expect(relay.frames()).toHaveLength(17);
    while (relay.pendingAcks.size) relay.acknowledge(relay.pendingAcks.values().next().value!);
    expect(relay.frames().map((event) => event.binaryBase64)).toEqual(
      frames.map((frame) => frame.toString("base64")),
    );
    expect(relay.maxPendingAcks()).toBe(16);

    // Once the queue drains, socket reads must resume for later output.
    relay.burst(frames.slice(0, 1));
    await vi.waitFor(() => expect(relay.frames()).toHaveLength(33));
  });

  it("still closes a stalled renderer and releases its queued output", async () => {
    const relay = await openRelay();
    relay.manualAck();
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    relay.burst(terminalFrames(32));
    await vi.waitFor(() => expect(relay.frames()).toHaveLength(16));
    expect(relay.closes()).toEqual([]);
    await vi.advanceTimersByTimeAsync(5_000);
    expect(relay.closes()).toEqual([{ sessionId, kind: "close", error: "Relay renderer stalled" }]);
    expect(relay.frames()).toHaveLength(16);
    expect(relay.account.closeVisit).toHaveBeenCalledExactlyOnceWith("test-visit");
    expect(relay.owner.listenerCount("destroyed")).toBe(0);
    expect(relay.owner.listenerCount("did-start-navigation")).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
  });
});

// Reuse the wire-level center double so the renderer hello/Host accept path is exercised too.
import { hubFixture, HOST } from "../../../../packages/client/test/hub.js";
import type { HubLink } from "@ait/client/internal/hub";

describe("desktop Hub data connections", () => {
  it.each(["instance", null, "stale-instance"])(
    "opens through presence with Relay instance %s and preserves the logical session during resume",
    async (instanceId) => {
      const h = hubFixture();
      const socket = await h.online();
      const account = {
        openVisit: vi.fn(),
        closeVisit: vi.fn(),
        snapshot: () => ({
          hosts: [{ host_id: HOST.host_id, server_id: "server", instance_id: instanceId }],
        }),
      };
      const manager = new AccountTransportManager(
        account as unknown as AccountSessionManager,
        h.link as unknown as HubLink,
      );
      const events: Record<string, unknown>[] = [];
      const owner = Object.assign(new EventEmitter(), {
        isDestroyed: () => false,
        send: (_channel: string, event: Record<string, unknown>) => {
          events.push(event);
          if (event.kind === "open")
            void manager.send(owner, "account-hub", { text: JSON.stringify({ type: "hello" }) });
        },
      });
      await manager.open(owner, "account-hub", HOST.host_id, h.link.centerUrl!);
      await h.flush();
      expect(account.openVisit).not.toHaveBeenCalled();
      const open = socket.frames("open")[0];
      socket.receive(
        { t: "accept", seq: 2, s: open.s },
        JSON.stringify({
          type: "server_info",
          info: {
            server_id: "server",
            instance_id: "instance",
            features: ["ait-rust-single-v1"],
          },
        }),
      );
      expect(events.some((event) => typeof event.text === "string")).toBe(true);
      socket.drop();
      await h.flush();
      const resumed = h.sockets.at(-1)!;
      resumed.open();
      resumed.welcome({ resumed: true, recv: open.seq });
      expect(events.some((event) => event.kind === "close")).toBe(false);
      await manager.send(owner, "account-hub", { text: "request" });
      expect(resumed.frames("msg")[0]).toMatchObject({ s: open.s });
      owner.emit("destroyed");
      expect(resumed.frames("close")[0]).toMatchObject({ s: open.s });
      h.link.stop();
    },
  );

  it.each([
    { server_id: "wrong-host", instance_id: "current", features: ["ait-rust-single-v1"] },
    { server_id: "server", instance_id: "", features: ["ait-rust-single-v1"] },
    { server_id: "server", instance_id: "current", features: [] },
  ])("rejects a Hub accept with invalid identity or protocol: %j", async (info) => {
    const h = hubFixture();
    const socket = await h.online();
    const account = {
      snapshot: () => ({
        hosts: [{ host_id: HOST.host_id, server_id: "server", instance_id: null }],
      }),
      openVisit: vi.fn(),
      closeVisit: vi.fn(),
    };
    const manager = new AccountTransportManager(
      account as unknown as AccountSessionManager,
      h.link as unknown as HubLink,
    );
    const events: Record<string, unknown>[] = [];
    const owner = Object.assign(new EventEmitter(), {
      isDestroyed: () => false,
      send: (_channel: string, event: Record<string, unknown>) => {
        events.push(event);
        if (event.kind === "open")
          void manager.send(owner, "account-hub", { text: JSON.stringify({ type: "hello" }) });
      },
    });
    await manager.open(owner, "account-hub", HOST.host_id, h.link.centerUrl!);
    await h.flush();
    const open = socket.frames("open")[0];
    socket.receive(
      { t: "accept", seq: 2, s: open.s },
      JSON.stringify({ type: "server_info", info }),
    );
    expect(
      events.some(
        (event) =>
          event.kind === "close" && event.error === "Target Host identity or protocol changed",
      ),
    ).toBe(true);
    expect(events.some((event) => event.kind === "message")).toBe(false);
    h.link.stop();
  });
});

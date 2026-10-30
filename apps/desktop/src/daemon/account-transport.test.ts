import { EventEmitter } from "node:events";
import { encodeTerminalStreamFrame, TerminalStreamOpcode } from "@ait/protocol/binary-frames/index";
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

async function openHub(synchronousAck = false) {
  const h = hubFixture();
  const socket = await h.online();
  const account = {
    snapshot: () => ({ hosts: [{ host_id: HOST.host_id, server_id: "test-server" }] }),
  };
  const manager = new AccountTransportManager(
    account as unknown as AccountSessionManager,
    h.link as unknown as HubLink,
  );
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
  cleanups.push(async () => {
    for (const scheduled of scheduledAcks) clearImmediate(scheduled);
    manager.closeAll();
    h.link.stop();
  });
  await manager.open(owner, sessionId, HOST.host_id, h.link.centerUrl!);
  await h.flush();
  const open = socket.frames("open")[0];
  socket.receive(
    { t: "accept", seq: 2, s: open.s },
    JSON.stringify({
      type: "server_info",
      info: {
        server_id: "test-server",
        instance_id: "test-instance",
        features: ["ait-rust-single-v1"],
      },
    }),
  );
  let sequence = 2;
  const burst = (frames: Buffer[]) => {
    for (const frame of frames)
      socket.receive({ t: "msg", seq: ++sequence, s: open.s, pt: "binary" }, frame);
  };
  await vi.waitFor(() => {
    expect(events.some((event) => typeof event.text === "string")).toBe(true);
    expect(pendingAcks.size).toBe(0);
  });
  return {
    manager,
    owner,
    link: h.link,
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

describe("desktop Hub terminal delivery", () => {
  it.each([16, 17, 32, 256])(
    "keeps the host connected and preserves all %i frames with asynchronous renderer ACKs",
    async (frameCount) => {
      const relay = await openHub();
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
    const relay = await openHub(true);
    const frames = terminalFrames(32);
    relay.burst(frames);
    await vi.waitFor(() => expect(relay.frames()).toHaveLength(frames.length));
    expect(relay.closes()).toEqual([]);
    expect(relay.frames().map((event) => event.binaryBase64)).toEqual(
      frames.map((frame) => frame.toString("base64")),
    );
  });

  it("delivers queued frames only as the renderer acknowledges its delivery window", async () => {
    const relay = await openHub();
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

    // Once the queue drains, later Hub output must still arrive.
    relay.burst(frames.slice(0, 1));
    await vi.waitFor(() => expect(relay.frames()).toHaveLength(33));
  });

  it("still closes a stalled renderer and releases its queued output", async () => {
    const relay = await openHub();
    relay.manualAck();
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    relay.burst(terminalFrames(32));
    await vi.waitFor(() => expect(relay.frames()).toHaveLength(16));
    expect(relay.closes()).toEqual([]);
    await vi.advanceTimersByTimeAsync(5_000);
    expect(relay.closes()).toEqual([{ sessionId, kind: "close", error: "Relay renderer stalled" }]);
    expect(relay.frames()).toHaveLength(16);
    relay.link.stop();
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

describe("desktop Hub availability", () => {
  it.each(["absent", "stopped", "other-center", "no-serve", "timeout"])(
    "fails explicitly when Hub is unavailable: %s",
    async (mode) => {
      const h = hubFixture();
      const account = {
        snapshot: () => ({ hosts: [{ host_id: HOST.host_id, server_id: "server" }] }),
      };
      const manager = new AccountTransportManager(
        account as unknown as AccountSessionManager,
        mode === "absent" ? undefined : (h.link as unknown as HubLink),
      );
      cleanups.push(async () => {
        manager.closeAll();
        h.link.stop();
      });
      if (mode === "other-center" || mode === "no-serve") {
        const socket = await h.online();
        if (mode === "no-serve") socket.presence(2, []);
      }
      if (mode === "timeout") h.link.start();
      const events: Record<string, unknown>[] = [];
      const owner = Object.assign(new EventEmitter(), {
        isDestroyed: () => false,
        send: (_channel: string, event: Record<string, unknown>) => events.push(event),
      });
      const opened = manager.open(
        owner,
        "account-unavailable",
        HOST.host_id,
        mode === "other-center" ? "https://other.test/api" : "https://center.test/api",
      );
      if (mode === "timeout") {
        await vi.advanceTimersByTimeAsync(4999);
        expect(events).toEqual([]);
        await vi.advanceTimersByTimeAsync(1);
      }
      await opened;
      expect(events).toEqual([
        {
          sessionId: "account-unavailable",
          kind: "close",
          error:
            mode === "no-serve"
              ? "Target Host is unavailable on Hub."
              : "Online Hub is unavailable.",
        },
      ]);
      expect(owner.listenerCount("destroyed")).toBe(0);
      expect(owner.listenerCount("did-start-navigation")).toBe(0);
    },
  );
});

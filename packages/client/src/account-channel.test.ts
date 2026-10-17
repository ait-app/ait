import { afterEach, expect, it, vi } from "vitest";
import { AccountChannel } from "./account-channel.js";
import { channelFixture, welcome } from "../test/account-channel.js";

afterEach(() => vi.useRealTimers());

function fixture(auto = true) {
  vi.useFakeTimers();
  const sockets = channelFixture(() => [], auto);
  const transport = sockets.create({ url: "wss://example.test/api/v1/relay/channel" });
  const handlers = { snapshot: vi.fn(), node: vi.fn(), failed: vi.fn() };
  const channel = new AccountChannel(transport, handlers);
  channel.start();
  return { channel, handlers, socket: sockets.sockets[0]! };
}

it("uses receipt time for repeated cached snapshots and continues application heartbeats", async () => {
  const { channel, handlers, socket } = fixture();
  await vi.advanceTimersByTimeAsync(180_000);
  expect(socket.transport.send).toHaveBeenCalledTimes(9);
  expect(handlers.failed).not.toHaveBeenCalled();
  expect(handlers.snapshot).toHaveBeenCalledTimes(10);
  channel.close();
  await vi.advanceTimersByTimeAsync(60_000);
  expect(socket.transport.send).toHaveBeenCalledTimes(9);
});

it("times out a silent server even while heartbeat writes succeed", async () => {
  const { channel, handlers, socket } = fixture();
  await vi.advanceTimersByTimeAsync(0);
  socket.reply = false;
  await vi.advanceTimersByTimeAsync(60_000);
  expect(handlers.failed).toHaveBeenCalledOnce();
  expect(socket.transport.close).toHaveBeenCalledOnce();
  channel.close();
});

it("ignores future server events and accepts node events before the first snapshot", () => {
  const { channel, handlers, socket } = fixture();
  socket.receive(welcome);
  socket.receive({ type: "future.event", extra: true });
  socket.receive({
    type: "node.online",
    node_id: "node",
    node_session_id: "session",
    host_id: null,
    observed_at: "old",
  });
  expect(handlers.failed).not.toHaveBeenCalled();
  expect(handlers.node).toHaveBeenCalledWith("node", true);
  expect(handlers.snapshot).not.toHaveBeenCalled();
  channel.close();
});

it.each([
  { type: "channel.welcome", protocol: "future" },
  { ...welcome, heartbeat_interval_seconds: 0 },
  { ...welcome, heartbeat_interval_seconds: 60 },
  { type: "hosts.snapshot", hosts: [], observed_at: "old" },
  { type: "channel.error", code: "heartbeat_timeout" },
])("closes invalid handshakes or server errors without exposing their payload: %j", (message) => {
  const { channel, handlers, socket } = fixture();
  socket.receive(message);
  expect(handlers.failed).toHaveBeenCalledOnce();
  channel.close();
});

it.each([
  ["{", false],
  ["binary", true],
])("rejects malformed or binary frames", (value, binary) => {
  const { channel, handlers, socket } = fixture();
  socket.raw(value, Boolean(binary));
  expect(handlers.failed).toHaveBeenCalledOnce();
  channel.close();
});

it("times out a connection that never sends welcome, despite unknown messages", async () => {
  const { channel, handlers, socket } = fixture(false);
  socket.receive({ type: "future.event" });
  await vi.advanceTimersByTimeAsync(10_000);
  expect(handlers.failed).toHaveBeenCalledOnce();
  channel.close();
});

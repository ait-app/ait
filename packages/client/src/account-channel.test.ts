import { afterEach, expect, it, vi } from "vitest";
import { AccountChannel } from "./account-channel.js";
import { channelFixture, welcome } from "../test/account-channel.js";

afterEach(() => vi.useRealTimers());

function fixture(auto = true) {
  vi.useFakeTimers();
  const sockets = channelFixture(() => [], auto);
  const transport = sockets.create({ url: "wss://example.test/api/v1/relay/channel" });
  const handlers = { snapshot: vi.fn(), online: vi.fn(), offline: vi.fn(), failed: vi.fn() };
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

it("verifies a silent foreground connection in three seconds without extending repeated checks", async () => {
  const { channel, handlers, socket } = fixture();
  await vi.advanceTimersByTimeAsync(0);
  socket.reply = false;
  channel.refresh({ verify: true });
  await vi.advanceTimersByTimeAsync(2_000);
  channel.refresh({ verify: true });
  expect(socket.transport.send).toHaveBeenCalledOnce();
  await vi.advanceTimersByTimeAsync(999);
  expect(handlers.failed).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(1);
  expect(handlers.failed).toHaveBeenCalledExactlyOnceWith(true);
  await vi.advanceTimersByTimeAsync(60_000);
  expect(socket.transport.close).toHaveBeenCalledOnce();
});

it("preserves a healthy channel when a cached snapshot answers foreground verification", async () => {
  const { channel, handlers, socket } = fixture();
  await vi.advanceTimersByTimeAsync(0);
  channel.refresh({ verify: true });
  // The fixture replies synchronously, using the same old observed_at.
  socket.reply = false;
  await vi.advanceTimersByTimeAsync(3_000);
  expect(handlers.failed).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(57_000);
  expect(handlers.failed).toHaveBeenCalledExactlyOnceWith(false);
});

it("bounds foreground recovery before welcome without sending a premature heartbeat", async () => {
  const { channel, handlers, socket } = fixture(false);
  channel.refresh({ verify: true });
  socket.receive({ type: "future.event" });
  await vi.advanceTimersByTimeAsync(3_000);
  expect(socket.transport.send).not.toHaveBeenCalled();
  expect(handlers.failed).toHaveBeenCalledExactlyOnceWith(true);
});

it("cancels pending foreground verification when the account closes the channel", async () => {
  const { channel, handlers, socket } = fixture();
  await vi.advanceTimersByTimeAsync(0);
  socket.reply = false;
  channel.refresh({ verify: true });
  channel.close();
  await vi.advanceTimersByTimeAsync(60_000);
  expect(handlers.failed).not.toHaveBeenCalled();
  expect(socket.transport.close).toHaveBeenCalledOnce();
});

it("ignores future server events and accepts node events before the first snapshot", () => {
  const { channel, handlers, socket } = fixture();
  socket.receive(welcome);
  socket.receive({ type: "future.event", extra: true });
  socket.receive({
    type: "node.online",
    node_id: "node",
    node_session_id: "session",
    host_id: "host",
    host: {
      host_id: "host",
      node_id: "node",
      server_id: "server",
      instance_id: "instance",
      name: "New host",
      platform: "linux",
      online: true,
      status: "offline",
      relay_modes: ["ait-rust-single-v1"],
    },
    observed_at: "old",
  });
  expect(handlers.failed).not.toHaveBeenCalled();
  expect(handlers.online).toHaveBeenCalledWith(
    expect.objectContaining({ host_id: "host", online: true, instance_id: "instance" }),
  );
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

it.each([
  { online: false },
  { instance_id: null },
  { server_id: null },
  { relay_modes: [] },
  { host_id: "wrong" },
  { node_id: "wrong" },
])("rejects online events without a consistent usable route: %j", (invalid) => {
  const { channel, handlers, socket } = fixture();
  socket.receive(welcome);
  socket.receive({
    type: "node.online",
    node_id: "node",
    node_session_id: "session",
    host_id: "host",
    observed_at: "old",
    host: {
      host_id: "host",
      node_id: "node",
      server_id: "server",
      instance_id: "instance",
      name: "Host",
      platform: "linux",
      online: true,
      status: "offline",
      relay_modes: ["ait-rust-single-v1"],
      ...invalid,
    },
  });
  expect(handlers.failed).toHaveBeenCalledOnce();
  expect(handlers.online).not.toHaveBeenCalled();
  channel.close();
});

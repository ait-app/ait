import { vi } from "vitest";
import type { DaemonTransport } from "../src/daemon-client-transport-types.js";

export const welcome = {
  type: "channel.welcome",
  protocol: "ait-channel-v1",
  connection_id: "connection",
  heartbeat_interval_seconds: 20,
  heartbeat_timeout_seconds: 60,
  host_snapshot_interval_seconds: 10,
};

export function channelFixture(hosts: () => unknown[] = () => [], auto = true) {
  const sockets: ReturnType<typeof createSocket>[] = [];
  function createSocket() {
    let message: (data: unknown, binary: boolean) => void = () => {};
    let closed = () => {};
    let error = () => {};
    const socket = {
      reply: true,
      receive: (value: unknown) => message(JSON.stringify(value), false),
      raw: (value: unknown, binary = false) => message(value, binary),
      disconnect: () => closed(),
      error: () => error(),
      snapshot: () =>
        socket.receive({
          type: "hosts.snapshot",
          hosts: hosts(),
          observed_at: "2026-10-16T09:00:00Z",
        }),
      transport: {
        send: vi.fn(() => {
          if (socket.reply) socket.snapshot();
        }),
        close: vi.fn(),
        onOpen: () => () => {},
        onMessage: (handler) => {
          message = handler;
          return () => {
            message = () => {};
          };
        },
        onClose: (handler) => {
          closed = handler;
          return () => {
            closed = () => {};
          };
        },
        onError: (handler) => {
          error = handler;
          return () => {
            error = () => {};
          };
        },
      } satisfies DaemonTransport,
    };
    return socket;
  }
  const create = vi.fn(
    (_options: { url: string; headers?: Record<string, string>; protocols?: string[] }) => {
      const socket = createSocket();
      sockets.push(socket);
      if (auto)
        queueMicrotask(() => {
          socket.receive(welcome);
          socket.snapshot();
        });
      return socket.transport;
    },
  );
  return { create, sockets };
}

/** Minimal browser/native constructor that exercises the production WebSocket adapters. */
export class ChannelWebSocket {
  static instances: ChannelWebSocket[] = [];
  readyState = 1;
  onmessage: ((event: unknown) => void) | null = null;
  onclose: ((event: unknown) => void) | null = null;
  onerror: ((event: unknown) => void) | null = null;
  onopen: ((event: unknown) => void) | null = null;
  constructor(
    readonly url: string,
    readonly protocols?: string[],
    readonly options?: { headers?: Record<string, string> },
  ) {
    ChannelWebSocket.instances.push(this);
    queueMicrotask(() => {
      this.onmessage?.({ data: JSON.stringify(welcome) });
      this.snapshot();
    });
  }
  private snapshot() {
    this.onmessage?.({
      data: JSON.stringify({ type: "hosts.snapshot", hosts: [], observed_at: "old" }),
    });
  }
  send = vi.fn(() => this.snapshot());
  close = vi.fn(() => {
    this.readyState = 3;
  });
}

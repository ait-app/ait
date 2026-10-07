import { expect, it, vi } from "vitest";
import { DaemonClient } from "../../../../../packages/client/src/daemon-client";
import { decodeFileTransferFrame, FileTransferOpcode } from "@ait/protocol/binary-frames/index";
import { CHANNEL_CAPABILITIES, createRustDaemonTransportFactory } from "./transport";
import { METHODS } from "./methods";
import type { Payload, TransportFactory } from "./types";

function harness(
  implemented = Object.values(METHODS).map((spec) => spec.method),
  url = "ws://127.0.0.1:7316/v1/ws",
) {
  const sockets: {
    send: ReturnType<typeof vi.fn>;
    close: ReturnType<typeof vi.fn>;
    open(): void;
    message(value: unknown, binary?: boolean): void;
    error(): void;
    end(): void;
  }[] = [];
  const factory: TransportFactory = () => {
    let onOpen = () => {};
    let onMessage = (_value: unknown, _binary: boolean) => {};
    let onError = () => {};
    let onClose = () => {};
    const socket = {
      send: vi.fn(),
      close: vi.fn(),
      open: () => onOpen(),
      message: (value: unknown, binary = false) =>
        onMessage(binary ? value : JSON.stringify(value), binary),
      error: () => onError(),
      end: () => onClose(),
    };
    sockets.push(socket);
    return {
      ...socket,
      onOpen: (fn) => {
        onOpen = fn;
        return () => {
          onOpen = () => {};
        };
      },
      onMessage: (fn) => {
        onMessage = fn;
        return () => {
          onMessage = () => {};
        };
      },
      onError: (fn) => {
        onError = fn;
        return () => {
          onError = () => {};
        };
      },
      onClose: (fn) => {
        onClose = fn;
        return () => {
          onClose = () => {};
        };
      },
    };
  };
  const base = vi.fn(factory);
  const transport = createRustDaemonTransportFactory(base)({
    url,
    headers: { Authorization: "Bearer test" },
    protocols: ["paseo.bearer.test"],
  });
  const received: Payload[] = [];
  const errors = vi.fn();
  const closed = vi.fn();
  transport.onMessage((value, binary) => {
    if (!binary) received.push(JSON.parse(String(value)));
  });
  transport.onError(errors);
  transport.onClose(closed);
  const stopHello = transport.onOpen(() =>
    transport.send(JSON.stringify({ type: "hello", clientId: "test" })),
  );
  function ready(version?: string, features: string[] = []) {
    for (const socket of sockets) socket.open();
    for (const [index, socket] of sockets.entries())
      socket.message({
        type: "server_info",
        info: {
          server_id: "server",
          version,
          instance_id: "instance",
          features,
          protocol: { major: 1, minor: 0 },
          implemented_capabilities: implemented,
        },
        negotiated_capabilities: CHANNEL_CAPABILITIES[index],
      });
  }
  function send(message: Payload) {
    transport.send(JSON.stringify({ type: "session", message }));
  }
  function last(channel: number) {
    return JSON.parse(sockets[channel].send.mock.lastCall![0]);
  }
  return {
    transport,
    stopHello,
    sockets,
    ready,
    send,
    last,
    received,
    base,
    errors,
    closed,
  };
}

function delayedUploads(h: ReturnType<typeof harness>, latencyMs: number, finalAckDelay = 0) {
  const metadata = new Map<string, Record<string, unknown>>();
  let dataBytes = 0;
  let ended = 0;
  for (const socket of h.sockets) {
    socket.send.mockImplementation((data: string | Uint8Array) => {
      socket.send.mockClear();
      if (typeof data === "string") {
        const request = JSON.parse(data);
        if (request.method === "connection.ping") {
          setTimeout(
            () =>
              socket.message({
                type: "response",
                request_id: request.request_id,
                result: { nonce: request.params.nonce },
              }),
            latencyMs,
          );
        }
        if (request.method === "file.upload.request")
          metadata.set(request.request_id, request.params);
        return;
      }
      const acknowledged = data[0] >= 0x40 && data[0] <= 0x42;
      if (!acknowledged && (data[0] < 0x10 || data[0] > 0x12)) return;
      const decoded = data.slice();
      if (acknowledged) decoded[0] -= 0x30;
      const frame = decodeFileTransferFrame(decoded)!;
      if (frame.opcode === FileTransferOpcode.FileChunk) dataBytes += frame.payload.byteLength;
      setTimeout(() => {
        if (frame.opcode === FileTransferOpcode.FileEnd) {
          ended++;
          const meta = metadata.get(frame.requestId)!;
          socket.message({
            type: "response",
            request_id: frame.requestId,
            result: {
              file: {
                type: "uploaded_file",
                id: "uploaded_" + frame.requestId,
                fileName: meta.fileName,
                mimeType: meta.mimeType,
                size: meta.size,
                path: "/tmp/qa-fixture",
              },
              error: null,
            },
          });
        }
        if (acknowledged) {
          setTimeout(
            () =>
              socket.message({
                type: "event",
                method: "connection.upload.ack",
                params: { requestId: frame.requestId, opcode: frame.opcode },
              }),
            frame.opcode === FileTransferOpcode.FileEnd ? finalAckDelay : 0,
          );
        }
      }, latencyMs);
    });
  }
  return { transferred: () => dataBytes, ended: () => ended, requests: () => [...metadata.keys()] };
}

it.each([
  { supported: false, latencyMs: 100 },
  { supported: true, latencyMs: 100 },
  { supported: true, latencyMs: 600 },
])(
  "80 MiB upload makes progress beyond ordinary RPC deadlines ($supported, $latencyMs ms)",
  async ({ supported, latencyMs }) => {
    vi.useFakeTimers();
    const h = harness();
    h.stopHello();
    const client = new DaemonClient({
      url: "ws://127.0.0.1:7316/v1/ws",
      clientId: "qa-large",
      transportFactory: () => h.transport,
      reconnect: { enabled: false },
    });
    try {
      const connected = client.connect();
      h.ready(undefined, supported ? ["client-message-chunks-v1"] : []);
      await connected;
      const stats = delayedUploads(h, latencyMs);
      const size = 80 * 1024 * 1024;
      let outcome: unknown;
      void client
        .uploadFile({
          requestId: "large-qa",
          fileName: "large.pdf",
          mimeType: "application/pdf",
          bytes: new Uint8Array(size),
        })
        .then(
          (value) => {
            outcome = { status: "fulfilled", value };
          },
          (error) => {
            outcome = { status: "rejected", message: error.message, code: error.code };
          },
        );
      await vi.advanceTimersByTimeAsync(642 * (latencyMs + 1) + 5000);
      expect(stats.transferred()).toBe(size);
      expect(stats.ended()).toBe(1);
      expect(outcome).toMatchObject({ status: "fulfilled" });
    } finally {
      await client.close();
      vi.useRealTimers();
    }
  },
);

it.each([false, true])(
  "QA regression: independent simultaneous file uploads should both finish (ACK feature=%s)",
  async (supported) => {
    vi.useFakeTimers();
    const h = harness();
    h.stopHello();
    const client = new DaemonClient({
      url: "ws://127.0.0.1:7316/v1/ws",
      clientId: "qa-parallel",
      transportFactory: () => h.transport,
      reconnect: { enabled: false },
    });
    try {
      const connected = client.connect();
      h.ready(undefined, supported ? ["client-message-chunks-v1"] : []);
      await connected;
      delayedUploads(h, 10, 50);
      const outcomes: { requestId: string; status: string; message?: string }[] = [];
      for (const requestId of ["first-qa", "second-qa"]) {
        void client
          .uploadFile({
            requestId,
            fileName: requestId,
            mimeType: "text/plain",
            bytes: new Uint8Array([1, 2]),
          })
          .then(
            () => {
              outcomes.push({ requestId, status: "fulfilled" });
            },
            (error) => {
              outcomes.push({ requestId, status: "rejected", message: error.message });
            },
          );
      }
      await vi.advanceTimersByTimeAsync(1_000);
      expect(outcomes.filter((result) => result.status === "fulfilled")).toHaveLength(2);
    } finally {
      await client.close();
      vi.useRealTimers();
    }
  },
);

it("a stalled ACK fails active and queued uploads without sending the queued file", async () => {
  vi.useFakeTimers();
  const h = harness();
  h.stopHello();
  const client = new DaemonClient({
    url: "ws://127.0.0.1:7316/v1/ws",
    clientId: "stalled",
    transportFactory: () => h.transport,
    reconnect: { enabled: false },
  });
  try {
    const connected = client.connect();
    h.ready(undefined, ["client-message-chunks-v1"]);
    await connected;
    const stats = delayedUploads(h, 45_000);
    const outcomes: string[] = [];
    for (const requestId of ["stalled", "queued"]) {
      void client
        .uploadFile({
          requestId,
          fileName: requestId,
          mimeType: "text/plain",
          bytes: new Uint8Array([1]),
        })
        .then(
          () => outcomes.push("success"),
          () => outcomes.push("failure"),
        );
    }
    await vi.advanceTimersByTimeAsync(31_000);
    expect(outcomes).toEqual(["failure", "failure"]);
    expect(stats.requests()).toEqual(["stalled"]);
    expect(stats.ended()).toBe(0);
  } finally {
    await client.close();
    vi.useRealTimers();
  }
});

it("bounds queued files while retaining the active upload", async () => {
  vi.useFakeTimers();
  const h = harness();
  h.stopHello();
  const client = new DaemonClient({
    url: "ws://127.0.0.1:7316/v1/ws",
    clientId: "bounded",
    transportFactory: () => h.transport,
    reconnect: { enabled: false },
  });
  try {
    const connected = client.connect();
    h.ready(undefined, ["client-message-chunks-v1"]);
    await connected;
    const failures: string[] = [];
    for (let index = 0; index < 17; index++) {
      void client
        .uploadFile({
          requestId: `queued-${index}`,
          fileName: "small",
          mimeType: "text/plain",
          bytes: new Uint8Array([1]),
        })
        .catch((error: Error) => {
          failures.push(error.message);
        });
    }
    await vi.advanceTimersByTimeAsync(1);
    expect(failures).toEqual(["File upload queue capacity exceeded"]);
  } finally {
    await client.close();
    vi.useRealTimers();
  }
});

import { describe, expect, it, vi } from "vitest";
import { DaemonClient } from "@ait/client/internal/daemon-client";
import {
  decodeFileTransferFrame,
  encodeFileTransferFrame,
  FileTransferOpcode,
} from "@ait/protocol/binary-frames/index";
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
  function ready(version?: string) {
    for (const socket of sockets) socket.open();
    for (const [index, socket] of sockets.entries())
      socket.message({
        type: "server_info",
        info: {
          server_id: "server",
          version,
          instance_id: "instance",
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

describe("Rust protocol adapter", () => {
  it("carries typed online service control through the SDK without replaying a grant", async () => {
    const h = harness();
    h.stopHello();
    const client = new DaemonClient({
      url: "ws://127.0.0.1:7316/v1/ws",
      clientId: "online-service-test",
      transportFactory: () => h.transport,
      reconnect: { enabled: false },
    });
    const result = {
      serverId: "server",
      instanceId: "instance",
      platform: "linux",
      status: { online: false, connecting: false, epoch: null, error: null },
    };
    try {
      const connected = client.connect();
      h.ready();
      await connected;
      expect(client.getLastServerInfoMessage()?.features?.onlineServiceSync).toBe(true);
      const status = client.getOnlineServiceStatus();
      const statusRequest = h.last(1);
      expect(statusRequest.method).toBe("relay.status.request");
      h.sockets[1].message({
        type: "response",
        request_id: statusRequest.request_id,
        method: statusRequest.method,
        result,
      });
      expect(await status).toMatchObject(result);
      const grant = {
        center_url: "https://example.test/api",
        control_ticket: "a".repeat(64),
        node_session_id: "00000000-0000-4000-8000-000000000001",
      };
      const start = client.connectOnlineService(grant);
      const startRequest = h.last(1);
      expect(startRequest).toMatchObject({ method: "relay.start.request", params: grant });
      h.sockets[1].message({
        type: "response",
        request_id: startRequest.request_id,
        method: startRequest.method,
        result: { ...result, status: { ...result.status, connecting: true } },
      });
      expect((await start).status.connecting).toBe(true);
      const stop = client.disconnectOnlineService();
      const stopRequest = h.last(1);
      expect(stopRequest.method).toBe("relay.stop.request");
      h.sockets[1].message({
        type: "response",
        request_id: stopRequest.request_id,
        method: stopRequest.method,
        result,
      });
      expect((await stop).status.online).toBe(false);
      await client.close();
      await expect(client.connectOnlineService(grant)).rejects.toThrow();
      expect(
        h.sockets[1].send.mock.calls.filter(
          ([message]) => JSON.parse(message).method === "relay.start.request",
        ),
      ).toHaveLength(1);
    } finally {
      await client.close();
    }
  });

  it("passes the connected server's software version to the SDK", async () => {
    const h = harness();
    h.stopHello();
    const client = new DaemonClient({
      url: "ws://127.0.0.1:7316/v1/ws",
      clientId: "server-version-test",
      transportFactory: () => h.transport,
      reconnect: { enabled: false },
    });
    try {
      const connected = client.connect();
      h.ready("0.0.10");
      await connected;
      expect(client.getLastServerInfoMessage()?.version).toBe("0.0.10");
    } finally {
      await client.close();
    }
  });

  it("completes interleaved SDK file reads using the original request IDs", async () => {
    const h = harness();
    h.stopHello();
    const client = new DaemonClient({
      url: "ws://127.0.0.1:7316/v1/ws",
      clientId: "file-read-test",
      transportFactory: () => h.transport,
      reconnect: { enabled: false },
    });
    const completed = vi.fn();
    const failed = vi.fn();
    try {
      const connected = client.connect();
      h.ready();
      await connected;
      const first = client.readFile("/workspace", "one.rs", "file-one");
      const firstId = h.last(2).request_id;
      const second = client.readFile("/workspace", "two.rs", "file-two-longer");
      const secondId = h.last(2).request_id;
      void Promise.all([first, second]).then(completed, failed);
      expect([firstId, secondId]).toEqual(["file-one", "file-two-longer"]);
      for (const id of [firstId, secondId]) {
        h.sockets[2].message(
          encodeFileTransferFrame({
            opcode: FileTransferOpcode.FileBegin,
            requestId: id,
            metadata: {
              mime: "text/plain",
              size: 3,
              encoding: "utf-8",
              modifiedAt: "now",
            },
          }).buffer,
          true,
        );
      }
      for (const [id, payload] of [
        [secondId, "two"],
        [firstId, "one"],
      ]) {
        h.sockets[2].message(
          encodeFileTransferFrame({
            opcode: FileTransferOpcode.FileChunk,
            requestId: id,
            payload,
          }),
          true,
        );
        h.sockets[2].message(
          encodeFileTransferFrame({
            opcode: FileTransferOpcode.FileEnd,
            requestId: id,
          }),
          true,
        );
      }
      await vi.waitFor(() => expect(completed).toHaveBeenCalledOnce());
      expect(failed).not.toHaveBeenCalled();
      expect(
        completed.mock.calls[0][0].map((file: { bytes: Uint8Array }) =>
          new TextDecoder().decode(file.bytes),
        ),
      ).toEqual(["one", "two"]);
      expect(client.isConnected).toBe(true);
    } finally {
      await client.close();
    }
  });

  it("releases binary read admission and timers at FileEnd and ignores late frames", () => {
    vi.useFakeTimers();
    const h = harness();
    const received = vi.fn();
    h.transport.onMessage((data, binary) => {
      if (binary) received(data);
    });
    try {
      h.ready();
      for (let index = 0; index < 260; index++) {
        const requestId = `file-${index}`;
        h.send({
          type: "fs.explorer.request",
          requestId,
          cwd: "/workspace",
          path: "empty.txt",
          mode: "file",
          acceptBinary: true,
        });
        const id = h.last(2).request_id;
        h.sockets[2].message(
          encodeFileTransferFrame({
            opcode: FileTransferOpcode.FileBegin,
            requestId: id,
            metadata: {
              mime: "text/plain",
              size: 0,
              encoding: "utf-8",
              modifiedAt: "now",
            },
          }),
          true,
        );
        const end = encodeFileTransferFrame({
          opcode: FileTransferOpcode.FileEnd,
          requestId: id,
        });
        h.sockets[2].message(end, true);
        expect(received.mock.lastCall![0]).toBe(end);
        expect(decodeFileTransferFrame(received.mock.lastCall![0])).toMatchObject({
          requestId,
        });
        const count = received.mock.calls.length;
        h.sockets[2].message(end, true);
        expect(received).toHaveBeenCalledTimes(count);
      }
      vi.advanceTimersByTime(300_001);
      expect(h.received).toHaveLength(1); // No completed read becomes a later timeout.
      expect(h.errors).not.toHaveBeenCalled();
    } finally {
      h.transport.close();
      vi.useRealTimers();
    }
  });

  it("discards file data arriving after a correlated read failure", () => {
    const h = harness();
    const binary = vi.fn();
    h.transport.onMessage((data, isBinary) => {
      if (isBinary) binary(data);
    });
    try {
      h.ready();
      h.send({
        type: "fs.explorer.request",
        requestId: "failed-read",
        cwd: "/workspace",
        path: "removed.rs",
        mode: "file",
        acceptBinary: true,
      });
      const id = h.last(2).request_id;
      h.sockets[2].message({
        type: "response",
        request_id: id,
        result: {
          cwd: "/workspace",
          path: "removed.rs",
          mode: "file",
          directory: null,
          file: null,
          error: "File disappeared",
        },
      });
      expect(h.received.at(-1)).toMatchObject({
        message: {
          type: "fs.explorer.response",
          payload: { requestId: "failed-read", error: "File disappeared" },
        },
      });
      h.sockets[2].message(
        encodeFileTransferFrame({
          opcode: FileTransferOpcode.FileChunk,
          requestId: id,
          payload: "stale",
        }),
        true,
      );
      expect(binary).not.toHaveBeenCalled();
      expect(h.errors).not.toHaveBeenCalled();
    } finally {
      h.transport.close();
    }
  });

  it("rejects a binary read response on a different physical connection", () => {
    const h = harness();
    try {
      h.ready();
      h.send({
        type: "fs.explorer.request",
        requestId: "wrong-channel",
        cwd: "/workspace",
        path: "empty.rs",
        mode: "file",
        acceptBinary: true,
      });
      h.sockets[1].message(
        encodeFileTransferFrame({
          opcode: FileTransferOpcode.FileEnd,
          requestId: h.last(2).request_id,
        }),
        true,
      );
      expect(h.errors).toHaveBeenCalledWith(
        expect.objectContaining({
          message: "Rust file came from the wrong connection",
        }),
      );
      expect(h.closed).toHaveBeenCalledOnce();
    } finally {
      h.transport.close();
    }
  });

  it("uses Rust negotiation and RPC for SSH channels", () => {
    const h = harness(undefined, "ait+desktop://ssh?host=build-box&daemonPort=7316");
    try {
      h.ready();
      expect(h.received).toHaveLength(1);
      expect(h.base).toHaveBeenCalledTimes(4);
      for (const [options] of h.base.mock.calls) {
        expect(options).toMatchObject({
          url: "ait+desktop://ssh?host=build-box&daemonPort=7316",
          headers: { Authorization: "Bearer test" },
        });
      }
      expect(h.last(0)).toMatchObject({
        type: "hello",
        protocol: { major: 1 },
      });
      h.send({ type: "project.list.request", requestId: "ssh-projects" });
      expect(h.last(1)).toMatchObject({ method: "project.list.request" });
    } finally {
      h.transport.close();
    }
  });

  it("uses canonical names and stays within Rust's per-connection limits", () => {
    expect(Object.keys(METHODS)).toHaveLength(171);
    for (const [name, spec] of Object.entries(METHODS)) expect(spec.method).toBe(name);
    expect(new Set(Object.values(METHODS).map((spec) => spec.method)).size).toBe(171);
    for (const capabilities of CHANNEL_CAPABILITIES) {
      expect(capabilities.length).toBeLessThanOrEqual(64);
      expect(capabilities.some((name) => /^(hub|chat|loop|plugin)[./]/.test(name))).toBe(false);
    }
    expect(Object.keys(METHODS).some((name) => /^(hub|chat|loop|plugin)[./]/.test(name))).toBe(
      false,
    );
  });

  it("routes the local workspace reset method on the Git channel", () => {
    const method = "checkout.reset_workspace.request";
    const h = harness([...Object.values(METHODS).map((spec) => spec.method), method]);
    try {
      h.ready();
      expect(CHANNEL_CAPABILITIES[2]).toContain(method);
      h.send({
        type: "checkout.reset_workspace.request",
        requestId: "reset-1",
        cwd: "/workspace",
        workspaceId: "workspace-1",
        initialBranch: "initial-workspace",
      });
      expect(h.last(2)).toMatchObject({
        method,
        params: {
          cwd: "/workspace",
          workspaceId: "workspace-1",
          initialBranch: "initial-workspace",
        },
      });
    } finally {
      h.transport.close();
    }
  });

  it("waits for every handshake and keeps credentials out of subprotocols and URLs", () => {
    const h = harness();
    try {
      h.ready();
      expect(h.received).toHaveLength(1);
      expect(h.received[0]).toMatchObject({
        type: "session",
        message: {
          type: "status",
          payload: {
            serverId: "server",
            features: { directorySync: false },
          },
        },
      });
      expect(h.base.mock.calls.every(([options]) => options.protocols === undefined)).toBe(true);
      expect(h.last(0)).toMatchObject({
        type: "hello",
        client_id: "test",
        protocol: { major: 1 },
      });
    } finally {
      h.transport.close();
    }
  });

  it("preserves native session search and workspace import targets", () => {
    const h = harness();
    try {
      h.ready();
      h.send({
        type: "provider.sessions.recent.list.request",
        requestId: "recent-sessions",
        cwd: "/repo",
        providers: ["codex"],
        query: "session title",
        limit: 15,
      });
      expect(h.last(METHODS["provider.sessions.recent.list.request"].channel)).toMatchObject({
        method: "provider.sessions.recent.list.request",
        params: { cwd: "/repo", providers: ["codex"], query: "session title", limit: 15 },
      });
      h.send({
        type: "agent.import.request",
        requestId: "import-session",
        providerId: "codex",
        providerHandleId: "native-session",
        cwd: "/repo",
        workspaceId: "wks_0123456789abcdef",
      });
      expect(h.last(METHODS["agent.import.request"].channel)).toMatchObject({
        method: "agent.import.request",
        params: {
          providerId: "codex",
          providerHandleId: "native-session",
          cwd: "/repo",
          workspaceId: "wks_0123456789abcdef",
        },
      });
    } finally {
      h.transport.close();
    }
  });

  it("correlates concurrent replies and sends params without envelope fields", () => {
    const h = harness();
    try {
      h.ready();
      h.send({ type: "project.list.request", requestId: "projects" });
      const first = h.last(1);
      h.send({
        type: "workspace.list.request",
        requestId: "workspaces",
      });
      const second = h.last(1);
      expect([first.request_id, second.request_id]).toEqual(["projects", "workspaces"]);
      expect(first.params).toEqual({});
      expect(second.method).toBe("workspace.list.request");
      h.sockets[1].message({
        type: "response",
        request_id: second.request_id,
        result: { entries: [] },
      });
      h.sockets[1].message({
        type: "response",
        request_id: first.request_id,
        result: { projects: [] },
      });
      expect(h.received.at(-1)).toMatchObject({
        message: {
          type: "project.list.response",
          payload: { requestId: "projects" },
        },
      });
      expect(h.received.at(-2)).toMatchObject({
        message: {
          type: "workspace.list.response",
          payload: { requestId: "workspaces" },
        },
      });
    } finally {
      h.transport.close();
    }
  });

  it("rejects a duplicate pending ID without replacing the original request", () => {
    const h = harness();
    try {
      h.ready();
      h.send({ type: "project.list.request", requestId: "shared-id" });
      expect(() => h.send({ type: "workspace.list.request", requestId: "shared-id" })).toThrow(
        "Duplicate pending Rust daemon request ID",
      );
      h.sockets[1].message({
        type: "response",
        request_id: "shared-id",
        result: { projects: [] },
      });
      expect(h.received.at(-1)).toMatchObject({
        message: { type: "project.list.response", payload: { requestId: "shared-id" } },
      });
      h.send({ type: "workspace.list.request", requestId: "shared-id" });
      expect(h.last(1).request_id).toBe("shared-id");
    } finally {
      h.transport.close();
    }
  });

  it("bootstraps SDK terminal subscriptions and routes updates without reconnecting", async () => {
    const h = harness();
    h.stopHello();
    const client = new DaemonClient({
      url: "ws://127.0.0.1:7316/v1/ws",
      clientId: "terminal-subscription-test",
      transportFactory: () => h.transport,
      reconnect: { enabled: false },
    });
    try {
      const connected = client.connect();
      h.ready();
      await connected;
      const subscription = client.observeTerminals({
        cwd: "/workspace",
        workspaceId: "workspace",
      });
      const update = vi.fn();
      subscription.subscribe({ snapshot: () => {}, update });
      const request = h.last(1);
      expect(request.method).toBe("terminal.list.subscribe.request");
      const snapshot = {
        subscriptionId: "terminal-list",
        cwd: "/workspace",
        workspaceId: "workspace",
        terminals: [],
      };
      h.sockets[1].message({
        type: "response",
        request_id: request.request_id,
        result: snapshot,
      });
      await expect(subscription.ready).resolves.toMatchObject(snapshot);
      h.sockets[1].message({
        type: "event",
        method: "terminal.list.changed",
        params: snapshot,
      });
      expect(update).toHaveBeenCalledWith({
        type: "terminal.list.changed",
        payload: snapshot,
      });
      expect(client.isConnected).toBe(true);
      expect(h.sockets.every((socket) => socket.close.mock.calls.length === 0)).toBe(true);
      const released = subscription.release();
      await vi.waitFor(() => expect(h.last(1).method).toBe("subscription.release.request"));
      h.sockets[1].message({
        type: "response",
        request_id: h.last(1).request_id,
        result: { subscriptionId: "terminal-list", released: true },
      });
      await released;
    } finally {
      await client.close();
    }
  });

  it("releases each subscription on the socket that owns it", () => {
    const h = harness();
    try {
      h.ready();
      h.send({
        type: "workspace.label.list.request",
        requestId: "labels",
        subscribe: {},
      });
      h.sockets[1].message({
        type: "response",
        request_id: h.last(1).request_id,
        result: { subscriptionId: "label-sub", labels: [] },
      });
      h.send({
        type: "subscription.release.request",
        requestId: "release",
        subscriptionId: "label-sub",
      });
      expect(h.last(1)).toMatchObject({
        method: "subscription.release.request",
        params: { subscriptionId: "label-sub" },
      });
    } finally {
      h.transport.close();
    }
  });

  it("delivers server errors as correlated SDK errors and exposes unavailable methods immediately", () => {
    const h = harness(["project.list.request", "connection.ping"]);
    try {
      h.ready();
      h.send({ type: "project.list.request", requestId: "list" });
      h.sockets[1].message({
        type: "error",
        request_id: h.last(1).request_id,
        code: "registry_io",
        message: "Registry failed",
      });
      expect(h.received.at(-1)).toMatchObject({
        message: {
          type: "rpc_error",
          payload: { requestId: "list", code: "registry_io" },
        },
      });
      h.send({ type: "schedule.list.request", requestId: "schedule" });
      expect(h.received.at(-1)).toMatchObject({
        message: {
          type: "rpc_error",
          payload: { requestId: "schedule", code: "not_implemented" },
        },
      });
    } finally {
      h.transport.close();
    }
  });

  it("preserves permission identity and voice stream ownership", () => {
    const h = harness();
    try {
      h.ready();
      h.send({
        type: "agent.permission.resolve.request",
        requestId: "permission",
        agentId: "agent",
        response: { behavior: "allow" },
      });
      expect(h.last(0).params.requestId).toBe("permission");
      h.send({
        type: "dictation.stream.start",
        dictationId: "dictation",
        format: "audio/wav",
      });
      expect(h.last(0)).toMatchObject({
        type: "event",
        method: "dictation.stream.start",
        params: { dictationId: "dictation" },
      });
      h.sockets[0].message({
        type: "event",
        method: "dictation.stream.ack",
        params: { dictationId: "dictation", ackSeq: -1 },
      });
      expect(h.received.at(-1)).toMatchObject({
        message: { type: "dictation.stream.ack" },
      });
    } finally {
      h.transport.close();
    }
  });

  it("routes terminal/file binary frames without corrupting them", () => {
    const h = harness();
    try {
      h.ready();
      const terminal = new Uint8Array([1, 2, 3]);
      const file = new Uint8Array([0x10, 2, 3]);
      h.transport.send(terminal);
      h.transport.send(file);
      expect(h.sockets[1].send).toHaveBeenLastCalledWith(terminal);
      expect(h.sockets[2].send).toHaveBeenLastCalledWith(file);
    } finally {
      h.transport.close();
    }
  });

  it("translates the SDK liveness ping and cleans every socket on partial failure", () => {
    const h = harness();
    h.ready();
    h.transport.send(JSON.stringify({ type: "connection.ping" }));
    const ping = h.last(0);
    expect(ping.request_id).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    );
    expect(ping.params.nonce).toBe(ping.request_id);
    h.sockets[0].message({
      type: "response",
      request_id: ping.request_id,
      result: { nonce: ping.params.nonce },
    });
    expect(h.received.at(-1)).toEqual({ type: "connection.pong" });
    h.sockets[2].end();
    expect(h.sockets.every((socket) => socket.close.mock.calls.length === 1)).toBe(true);
    expect(h.closed).toHaveBeenCalledTimes(1);
    expect(() => h.send({ type: "project.list.request" })).toThrow("closed");
  });

  it("rejects mixed server instances and never reports a successful connection", () => {
    const h = harness();
    for (const socket of h.sockets) socket.open();
    for (const index of [0, 1])
      h.sockets[index].message({
        type: "server_info",
        info: {
          server_id: "server",
          instance_id: String(index),
          protocol: { major: 1, minor: 0 },
          implemented_capabilities: [],
        },
        negotiated_capabilities: [],
      });
    expect(h.received).toHaveLength(0);
    expect(h.errors).toHaveBeenCalledTimes(1);
    expect(h.sockets.every((socket) => socket.close.mock.calls.length === 1)).toBe(true);
  });

  it("closes every socket when only part of the handshake completes", () => {
    vi.useFakeTimers();
    const h = harness();
    try {
      h.sockets[0].open();
      vi.advanceTimersByTime(10_001);
      expect(h.errors).toHaveBeenCalledTimes(1);
      expect(h.received).toHaveLength(0);
      expect(h.sockets.every((socket) => socket.close.mock.calls.length === 1)).toBe(true);
    } finally {
      h.transport.close();
      vi.useRealTimers();
    }
  });

  it("expires pending RPCs and ignores late replies", () => {
    vi.useFakeTimers();
    const h = harness();
    try {
      h.ready();
      h.send({ type: "project.list.request", requestId: "expired" });
      const id = h.last(1).request_id;
      vi.advanceTimersByTime(300_001);
      expect(h.received.at(-1)).toMatchObject({
        message: {
          type: "rpc_error",
          payload: { requestId: "expired", code: "timeout" },
        },
      });
      const count = h.received.length;
      h.sockets[1].message({
        type: "response",
        request_id: id,
        result: { projects: [] },
      });
      expect(h.received).toHaveLength(count);
    } finally {
      h.transport.close();
      vi.useRealTimers();
    }
  });

  it("preserves server identity and ownership in lifecycle status events", () => {
    const h = harness();
    try {
      h.ready();
      h.sockets[0].message({
        type: "event",
        method: "status.server_info",
        params: {
          subscriptionId: "lifecycle",
          info: { server_id: "server", version: "0.0.10", lifecycle: "draining" },
        },
      });
      expect(h.received.at(-1)).toMatchObject({
        message: {
          type: "status",
          payload: {
            status: "server_info",
            serverId: "server",
            version: "0.0.10",
            subscriptionId: "lifecycle",
          },
        },
      });
    } finally {
      h.transport.close();
    }
  });
});

describe("Browser host bridge", () => {
  it("preserves server request fields and uses the payload requestId for callbacks", () => {
    const h = harness();
    try {
      h.ready();
      const channel = METHODS["browser.host.register.request"].channel;
      h.send({
        type: "browser.host.register.request",
        requestId: "register",
        hostKind: "desktop",
        supportedCommands: ["list_tabs"],
      });
      const registered = h.last(channel);
      h.sockets[channel].message({
        type: "response",
        request_id: registered.request_id,
        result: { subscriptionId: "host-lease" },
      });
      h.sockets[channel].message({
        type: "event",
        method: "browser.automation.execute.request",
        params: {
          subscriptionId: "host-lease",
          requestId: "browser-call",
          command: { command: "list_tabs", args: {} },
          workspaceId: "workspace",
        },
      });
      expect(h.received.at(-1)).toEqual({
        type: "session",
        message: {
          type: "browser.automation.execute.request",
          subscriptionId: "host-lease",
          requestId: "browser-call",
          command: { command: "list_tabs", args: {} },
          workspaceId: "workspace",
        },
      });
      const payload = {
        requestId: "browser-call",
        ok: true,
        result: { command: "list_tabs", tabs: [] },
      };
      h.send({ type: "browser.automation.execute.response", payload });
      expect(h.last(channel)).toEqual({
        type: "response",
        method: "browser.automation.execute.response",
        request_id: "browser-call",
        params: payload,
      });
      h.send({
        type: "subscription.release.request",
        requestId: "release",
        subscriptionId: "host-lease",
      });
      expect(h.last(channel).method).toBe("subscription.release.request");
    } finally {
      h.transport.close();
    }
  });
});

describe("Rust admission retries", () => {
  it("retries a rejected busy read with the same correlation and returns its result", async () => {
    vi.useFakeTimers();
    const h = harness();
    try {
      h.ready();
      h.send({ type: "daemon.config.get.request", requestId: "config" });
      const wire = h.last(1);
      h.sockets[1].message({
        type: "error",
        request_id: wire.request_id,
        code: "resource_exhausted",
        message: "Busy",
        retryable: true,
      });
      expect(h.received).toHaveLength(1);
      await vi.advanceTimersByTimeAsync(100);
      expect(h.last(1)).toEqual(wire);
      h.sockets[1].message({
        type: "response",
        request_id: wire.request_id,
        result: { config: { appendSystemPrompt: "saved" } },
      });
      expect(h.received.at(-1)).toMatchObject({
        message: {
          type: "daemon.config.get.response",
          payload: {
            requestId: "config",
            config: { appendSystemPrompt: "saved" },
          },
        },
      });
    } finally {
      h.transport.close();
      vi.useRealTimers();
    }
  });

  it("bounds retries and cancels pending retry timers on close", async () => {
    vi.useFakeTimers();
    const h = harness();
    try {
      h.ready();
      h.send({ type: "daemon.config.get.request", requestId: "config" });
      const wire = h.last(1);
      for (let attempt = 0; attempt < 6; attempt++) {
        h.sockets[1].message({
          type: "error",
          request_id: wire.request_id,
          code: "resource_exhausted",
          message: "Busy",
          retryable: true,
        });
        await vi.advanceTimersByTimeAsync(2000);
      }
      expect(h.received.at(-1)).toMatchObject({
        message: {
          type: "rpc_error",
          payload: { code: "resource_exhausted" },
        },
      });
      h.send({
        type: "daemon.config.get.request",
        requestId: "cancelled",
      });
      h.sockets[1].message({
        type: "error",
        request_id: h.last(1).request_id,
        code: "resource_exhausted",
        message: "Busy",
        retryable: true,
      });
      const sent = h.sockets[1].send.mock.calls.length;
      h.transport.close();
      await vi.advanceTimersByTimeAsync(1000);
      expect(h.sockets[1].send).toHaveBeenCalledTimes(sent);
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      h.transport.close();
      vi.useRealTimers();
    }
  });

  it("keeps terminal event rejections in the session without reconnecting the host", () => {
    const h = harness();
    try {
      h.ready();
      expect(h.received[0]).toMatchObject({
        message: {
          payload: {
            features: {
              "terminal-restore-modes": true,
              "terminal-input-mode-replay": true,
              "terminal-size-ownership": true,
            },
          },
        },
      });
      h.sockets[1].message({
        type: "error",
        request_id: null,
        code: "invalid_message",
        message: "Invalid terminal parameters",
        retryable: false,
      });
      expect(h.errors).not.toHaveBeenCalled();
      expect(h.closed).not.toHaveBeenCalled();
      expect(h.sockets.every((socket) => socket.close.mock.calls.length === 0)).toBe(true);
      expect(h.received.at(-1)).toMatchObject({
        message: {
          type: "rpc_error",
          payload: {
            requestType: "terminal.input",
            code: "invalid_message",
            error: "Invalid terminal parameters",
          },
        },
      });
      h.send({
        type: "daemon.get_status.request",
        requestId: "still-alive",
      });
      const sent = h.last(1);
      h.sockets[1].message({
        type: "response",
        request_id: sent.request_id,
        result: { serverId: "server" },
      });
      expect(h.received.at(-1)).toMatchObject({
        message: { payload: { requestId: "still-alive" } },
      });
      h.sockets[1].error();
      expect(h.errors).toHaveBeenCalledOnce();
      expect(h.closed).toHaveBeenCalledOnce();
    } finally {
      h.transport.close();
    }
  });

  it("never retries a non-retryable error", async () => {
    vi.useFakeTimers();
    const h = harness();
    try {
      h.ready();
      h.send({ type: "daemon.config.get.request", requestId: "config" });
      const sent = h.sockets[1].send.mock.calls.length;
      h.sockets[1].message({
        type: "error",
        request_id: h.last(1).request_id,
        code: "resource_exhausted",
        message: "Busy",
        retryable: false,
      });
      await vi.advanceTimersByTimeAsync(5000);
      expect(h.sockets[1].send).toHaveBeenCalledTimes(sent);
      expect(h.received.at(-1)).toMatchObject({
        message: { type: "rpc_error" },
      });
    } finally {
      h.transport.close();
      vi.useRealTimers();
    }
  });
});

it("account relay negotiates and routes every capability through one business socket", () => {
  const h = harness(undefined, "ait+desktop://account-relay/00000000-0000-4000-8000-000000000001");
  expect(h.sockets).toHaveLength(1);
  h.sockets[0].open();
  expect(h.last(0).required_capabilities).toEqual(["connection.single.v1"]);
  expect(h.last(0).capabilities.length).toBeGreaterThan(64);
  h.sockets[0].message({
    type: "server_info",
    info: {
      server_id: "server",
      instance_id: "instance",
      protocol: { major: 1, minor: 0 },
      implemented_capabilities: Object.values(METHODS).map((spec) => spec.method),
    },
    negotiated_capabilities: CHANNEL_CAPABILITIES.flat(),
  });
  h.transport.send(new Uint8Array([1, 2, 3]));
  h.transport.send(new Uint8Array([0x10, 2, 3]));
  expect(h.sockets[0].send).toHaveBeenLastCalledWith(new Uint8Array([0x10, 2, 3]));
  h.transport.close();
});

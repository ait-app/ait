import { EventEmitter } from "node:events";
import { afterEach, describe, expect, it, vi } from "vitest";
import { hubFixture } from "../../test/hub.js";

/** Node's ws emits the failed-handshake error asynchronously, after close() returns. */
class ConnectingSocket extends EventEmitter {
  readyState = 0;
  send = vi.fn();
  close = vi.fn(() => {
    this.readyState = 2;
    setTimeout(() => {
      this.readyState = 3;
      this.emit("error", new Error("WebSocket was closed before the connection was established"));
      this.emit("close", 1006, "");
    }, 0);
  });
}

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("hub socket cleanup", () => {
  it("reports a timed-out handshake as unavailable and retries without an uncaught ws error", async () => {
    const sockets: ConnectingSocket[] = [];
    const h = hubFixture({
      webSocketFactory: () => {
        const socket = new ConnectingSocket();
        sockets.push(socket);
        return socket;
      },
    });
    const states: string[] = [];
    h.link.subscribe(() => states.push(h.link.state));
    h.link.start();
    await h.flush();

    await vi.advanceTimersByTimeAsync(10_001);
    expect(h.link.state).toBe("unavailable");
    expect(states).toEqual(["connecting", "unavailable"]);
    expect(h.logger.info).toHaveBeenCalledWith(
      expect.objectContaining({ reason: "handshake_timeout" }),
      "[Hub] connection lost",
    );
    expect(sockets[0].eventNames()).toEqual([]);

    await vi.advanceTimersByTimeAsync(300);
    expect(sockets).toHaveLength(2);
    expect(h.link.state).toBe("connecting");
    h.link.stop();
    await h.flush();
    expect(sockets[1].eventNames()).toEqual([]);
  });

  it("can stop during a handshake without an uncaught ws error or a reconnect", async () => {
    const socket = new ConnectingSocket();
    const factory = vi.fn(() => socket);
    const h = hubFixture({ webSocketFactory: factory });
    h.link.start();
    await h.flush();

    h.link.stop();
    await h.flush();
    expect(socket.eventNames()).toEqual([]);
    await vi.advanceTimersByTimeAsync(30_000);
    expect(h.link.state).toBe("stopped");
    expect(factory).toHaveBeenCalledOnce();
  });
});

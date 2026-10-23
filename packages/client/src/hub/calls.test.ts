import { afterEach, describe, expect, it, vi } from "vitest";
import { hubFixture } from "../../test/hub.js";
import { HubCallError } from "./calls.js";

afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});
async function online() {
  const fixture = hubFixture();
  fixture.link.start();
  await fixture.flush();
  const socket = fixture.socket();
  socket.open();
  socket.welcome({ center_methods: ["push.register", "session.heartbeat"] });
  return { ...fixture, socket };
}

describe("Hub center calls", () => {
  it("requires advertised capabilities on old centers", async () => {
    const h = hubFixture();
    await h.online();
    await expect(h.link.call("push.register", {})).rejects.toMatchObject({ code: "unsupported" });
    h.link.stop();
  });
  it("shares session sequence numbers and waits for reply after an ACK", async () => {
    const h = await online();
    const resolved = vi.fn();
    const call = h.link.call("push.register", { token: "test-only" }).then(resolved);
    const header = h.socket.frames("call")[0];
    const session = h.link.openSession("node");
    session.send("hello");
    expect(h.socket.frames("open")[0].seq).toBe(Number(header.seq) + 1);
    h.socket.receive({ t: "ack", a: header.seq });
    await h.flush();
    expect(resolved).not.toHaveBeenCalled();
    h.socket.receive(
      { t: "reply", seq: 1, id: header.id },
      JSON.stringify({ result: { expiresAt: "later" } }),
    );
    await call;
    expect(resolved).toHaveBeenCalledWith({ expiresAt: "later" });
    h.link.stop();
  });
  it("keeps pending outcomes through physical resume and deduplicates events", async () => {
    const h = await online();
    const events = vi.fn();
    h.link.onEvent(events);
    const call = h.link.call("session.heartbeat", { appVisible: true });
    const header = h.socket.frames("call")[0];
    h.socket.drop();
    await h.flush();
    const socket = h.sockets.at(-1)!;
    socket.open();
    socket.welcome({ resumed: true, recv: header.seq, center_methods: ["session.heartbeat"] });
    expect(socket.frames("call")).toEqual([]);
    const event = {
      t: "event",
      seq: 1,
      sub: "subscription",
      cursor: 55,
      host_id: "host",
      event_id: "event",
      method: "agent.update",
    };
    socket.receive(event, JSON.stringify({ id: "agent" }));
    socket.receive(event, JSON.stringify({ id: "agent" }));
    socket.receive({ t: "reply", seq: 2, id: header.id }, JSON.stringify({ result: {} }));
    await expect(call).resolves.toEqual({});
    expect(events).toHaveBeenCalledExactlyOnceWith(
      expect.objectContaining({ cursor: 55, hostId: "host", eventId: "event" }),
    );
    h.link.stop();
  });
  it("reports center errors without ending sessions", async () => {
    const h = await online();
    const call = h.link.call("push.register", {});
    const rejected = expect(call).rejects.toBeInstanceOf(HubCallError);
    h.socket.receive(
      { t: "reply", seq: 1, id: h.socket.frames("call")[0].id },
      JSON.stringify({ error: { code: "invalid_params", message: "Bad token" } }),
    );
    await rejected;
    expect(h.link.state).toBe("online");
    h.link.stop();
  });
  it("bounds pending calls and fails them on stop", async () => {
    const h = await online();
    const calls = Array.from({ length: 16 }, () => h.link.call("push.register", {}));
    const outcomes = calls.map((call) =>
      expect(call).rejects.toMatchObject({ code: "link_reset" }),
    );
    await expect(h.link.call("push.register", {})).rejects.toMatchObject({ code: "capacity" });
    h.link.stop();
    await Promise.all(outcomes);
  });
  it("does not confuse timeout with a rejected business operation", async () => {
    const h = await online();
    const result = expect(h.link.call("push.register", {})).rejects.toMatchObject({
      code: "timeout",
    });
    await vi.advanceTimersByTimeAsync(30_000);
    await result;
    h.link.stop();
  });
});

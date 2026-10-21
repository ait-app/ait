import { afterEach, describe, expect, it, vi } from "vitest";
import { CENTER, HOST, hubFixture, RESUME_TOKEN } from "../../test/hub.js";

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

/** Open an accepted session to the fixture Host. */
async function acceptedSession(h: ReturnType<typeof hubFixture>, socket = h.socket()) {
  const session = h.link.openSession(HOST.node_id);
  const events = { close: vi.fn(), error: vi.fn(), message: vi.fn() };
  session.onClose(events.close);
  session.onError(events.error);
  session.onMessage(events.message);
  await h.flush();
  session.send('{"type":"hello"}');
  socket.receive({ t: "accept", seq: 2, s: session.id }, '{"type":"server_info"}');
  return { session, events };
}

describe("hub link handshake", () => {
  it("connects with a Bearer ticket, says hello and becomes ready on presence", async () => {
    const h = hubFixture();
    const states: string[] = [];
    h.link.subscribe(() => states.push(h.link.state));
    h.link.start();
    await h.flush();
    expect(h.ticket).toHaveBeenCalledOnce();
    const socket = h.socket();
    expect(socket.url).toBe("wss://center.test/api/v1/hub");
    expect(socket.options).toEqual({ headers: { Authorization: `Bearer ${"0".repeat(63)}1` } });
    expect(socket.binaryType).toBe("arraybuffer");
    socket.open();
    expect(socket.frames()).toEqual([{ t: "hello", v: 1, resume: null }]);
    socket.welcome();
    expect(h.link.state).toBe("online");
    expect(h.link.ready).toBe(false);
    socket.presence(1, [HOST]);
    expect(h.link.ready).toBe(true);
    expect(h.link.centerUrl).toBe(CENTER);
    expect(h.link.serveFor(HOST.host_id)).toMatchObject({ node_id: HOST.node_id });
    expect(h.link.serveFor("other")).toBeUndefined();
    expect(states).toEqual(["connecting", "online", "online"]);
  });

  it("carries the credential as a subprotocol for browsers", async () => {
    const h = hubFixture({ authentication: "subprotocol" });
    h.link.start();
    await h.flush();
    expect(h.socket().options).toEqual({ protocols: [`ait.ticket.${"0".repeat(63)}1`] });
  });

  it("waits for a pending link and gives up on an unavailable center", async () => {
    const h = hubFixture();
    h.link.start();
    const ready = h.link.whenReady(5000);
    await h.flush();
    h.socket().open();
    h.socket().welcome();
    h.socket().presence(1, []);
    await expect(ready).resolves.toBe(true);

    const old = hubFixture();
    old.ticket.mockRejectedValue(Object.assign(new Error("Not found"), { status: 404 }));
    old.link.start();
    const waiting = old.link.whenReady(5000);
    await old.flush();
    expect(old.link.state).toBe("unavailable");
    await expect(waiting).resolves.toBe(false);
    await expect(old.link.whenReady(5000)).resolves.toBe(false);
    // A center without the hub is asked again only after minutes, not seconds.
    await vi.advanceTimersByTimeAsync(60_000);
    expect(old.ticket).toHaveBeenCalledOnce();
  });
});

describe("hub link sequencing", () => {
  it("numbers outbound frames and coalesces acknowledgements", async () => {
    const h = hubFixture();
    const socket = await h.online();
    const { session, events } = await acceptedSession(h);
    expect(events.message).toHaveBeenCalledWith('{"type":"server_info"}', false);
    session.send("text");
    session.send(new Uint8Array([1, 2]));
    expect(socket.frames().slice(1)).toEqual([
      { t: "open", seq: 1, s: session.id, to: { node: HOST.node_id } },
      { t: "msg", seq: 2, s: session.id, pt: "text" },
      { t: "msg", seq: 3, s: session.id, pt: "binary" },
    ]);
    expect(new TextDecoder().decode(socket.sent[1].payload)).toBe('{"type":"hello"}');
    socket.receive({ t: "msg", seq: 3, s: session.id, pt: "text" }, "a");
    socket.receive({ t: "msg", seq: 4, s: session.id, pt: "binary" }, new Uint8Array([9]));
    expect(socket.frames("ack")).toEqual([]);
    await vi.advanceTimersByTimeAsync(50);
    expect(socket.frames("ack")).toEqual([{ t: "ack", a: 4 }]);
    expect(events.message).toHaveBeenCalledWith("a", false);
    expect(events.message).toHaveBeenLastCalledWith(new Uint8Array([9]), true);
  });

  it("resumes with the resume token, resends unacknowledged frames and keeps sessions open", async () => {
    const h = hubFixture();
    const first = await h.online();
    const { session, events } = await acceptedSession(h);
    session.send("one");
    session.send("two");
    first.receive({ t: "ack", a: 2 });
    first.drop();
    expect(h.link.state).toBe("reconnecting");
    // Sent while offline: buffered and delivered after the resume.
    session.send("three");
    await h.flush();
    const second = h.socket();
    expect(second).not.toBe(first);
    expect(h.ticket).toHaveBeenCalledOnce();
    expect(second.options).toEqual({ headers: { Authorization: `Bearer ${RESUME_TOKEN}` } });
    second.open();
    expect(second.frames()).toEqual([{ t: "hello", v: 1, resume: { link: "link-1", recv: 2 } }]);
    second.welcome({ resumed: true, recv: 2, resume_token: undefined });
    expect(second.frames().slice(1)).toEqual([
      { t: "msg", seq: 3, s: session.id, pt: "text" },
      { t: "msg", seq: 4, s: session.id, pt: "text" },
    ]);
    expect(second.sent.slice(1).map((frame) => new TextDecoder().decode(frame.payload))).toEqual([
      "two",
      "three",
    ]);
    // A replayed frame is acknowledged again but not delivered twice.
    second.receive({ t: "accept", seq: 2, s: session.id }, "{}");
    second.receive({ t: "msg", seq: 3, s: session.id, pt: "text" }, "missed");
    expect(events.message).toHaveBeenCalledTimes(2);
    expect(events.message).toHaveBeenLastCalledWith("missed", false);
    expect(events.close).not.toHaveBeenCalled();
    expect(events.error).not.toHaveBeenCalled();
    expect(session.closed).toBe(false);
    expect(h.link.ready).toBe(true);
  });

  it("falls back to a fresh ticket on resume_expired and fails the old sessions", async () => {
    const h = hubFixture();
    const first = await h.online();
    const { session, events } = await acceptedSession(h);
    first.drop();
    await h.flush();
    const resumed = h.socket();
    resumed.open();
    resumed.receive({ t: "error", code: "resume_expired" });
    expect(events.error).toHaveBeenCalledWith(new Error("Hub link reset (resume_expired)."));
    expect(events.close).toHaveBeenCalledWith({
      code: 1006,
      reason: "Hub link reset (resume_expired).",
    });
    expect(session.closed).toBe(true);
    expect(resumed.close).toHaveBeenCalled();
    await h.flush();
    expect(h.ticket).toHaveBeenCalledTimes(2);
    const fresh = h.socket();
    expect(fresh.options).toEqual({ headers: { Authorization: `Bearer ${"0".repeat(63)}2` } });
    fresh.open();
    expect(fresh.frames()).toEqual([{ t: "hello", v: 1, resume: null }]);
    fresh.welcome({ link: "link-2" });
    fresh.presence(1, [HOST]);
    expect(h.link.ready).toBe(true);
    const next = h.link.openSession(HOST.node_id);
    await h.flush();
    next.send('{"type":"hello"}');
    expect(fresh.frames("open")).toEqual([
      { t: "open", seq: 1, s: next.id, to: { node: HOST.node_id } },
    ]);
  });

  it("switches to a ticket after resume-token connections fail and fails sessions on a new link", async () => {
    const h = hubFixture();
    const first = await h.online();
    const { events } = await acceptedSession(h);
    first.drop();
    await h.flush();
    h.socket().drop();
    await vi.advanceTimersByTimeAsync(300);
    h.socket().drop();
    await vi.advanceTimersByTimeAsync(600);
    const ticketed = h.socket();
    expect(h.ticket).toHaveBeenCalledTimes(2);
    ticketed.open();
    expect(ticketed.frames("hello")).toEqual([
      { t: "hello", v: 1, resume: { link: "link-1", recv: 2 } },
    ]);
    // The center no longer had the link: a fresh link replaces it.
    ticketed.welcome({ link: "link-2" });
    expect(events.close).toHaveBeenCalledWith({
      code: 1006,
      reason: "Hub link reset (link_reset).",
    });
    expect(h.link.presence).toBeNull();
  });

  it("fails sessions once the grace period passes without a resume", async () => {
    const h = hubFixture();
    const first = await h.online();
    const { events } = await acceptedSession(h);
    h.factory.mockImplementation(() => {
      throw new Error("offline");
    });
    first.drop();
    await vi.advanceTimersByTimeAsync(119_000);
    expect(events.close).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1000);
    expect(events.close).toHaveBeenCalledWith({
      code: 1006,
      reason: "Hub link reset (grace_expired).",
    });
    expect(h.link.state).not.toBe("reconnecting");
  });
});

describe("hub link liveness", () => {
  it("verifies with ping/pong and reconnects when no pong arrives", async () => {
    const h = hubFixture();
    const socket = await h.online();
    const alive = h.link.verify();
    expect(socket.frames("ping")).toEqual([{ t: "ping", n: 1 }]);
    socket.receive({ t: "pong", n: 1 });
    await expect(alive).resolves.toBe(true);

    const dead = h.link.verify(3000);
    await vi.advanceTimersByTimeAsync(3000);
    await expect(dead).resolves.toBe(false);
    expect(socket.close).toHaveBeenCalled();
    expect(h.link.state).toBe("reconnecting");
    // Fake timers defer zero-delay timers created inside a timer callback by 1 ms.
    await vi.advanceTimersByTimeAsync(1);
    expect(h.sockets).toHaveLength(2);
    expect(h.socket().options).toEqual({ headers: { Authorization: `Bearer ${RESUME_TOKEN}` } });
  });

  it("starts a backed-off reconnect at once when verified in the foreground", async () => {
    const h = hubFixture();
    const first = await h.online();
    first.drop();
    await h.flush();
    h.socket().drop();
    expect(h.sockets).toHaveLength(2);
    await expect(h.link.verify()).resolves.toBe(false);
    await h.flush();
    expect(h.sockets).toHaveLength(3);
  });

  it("stop closes the peer sessions, the socket and forgets the link", async () => {
    const h = hubFixture();
    const socket = await h.online();
    const { session, events } = await acceptedSession(h);
    h.link.stop();
    expect(socket.frames("close")).toEqual([{ t: "close", seq: 2, s: session.id, code: "closed" }]);
    expect(socket.close).toHaveBeenCalled();
    expect(events.close).toHaveBeenCalledOnce();
    expect(h.link.state).toBe("stopped");
    expect(h.link.ready).toBe(false);
    expect(() => h.link.openSession(HOST.node_id)).toThrow("stopped");
    h.link.start();
    await h.flush();
    h.socket().open();
    expect(h.socket().frames()).toEqual([{ t: "hello", v: 1, resume: null }]);
  });
});

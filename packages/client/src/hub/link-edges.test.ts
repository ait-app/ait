import { afterEach, describe, expect, it, vi } from "vitest";
import { HOST, hubFixture } from "../../test/hub.js";
import { HubLink } from "./link.js";

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("hub link failures", () => {
  it("reconnects after a socket error or a missing welcome", async () => {
    const h = hubFixture();
    const socket = await h.online();
    socket.fail();
    expect(h.link.state).toBe("reconnecting");
    expect(socket.close).toHaveBeenCalled();
    await h.flush();
    expect(h.sockets).toHaveLength(2);
    expect(h.socket().options?.headers?.Authorization).toBe(`Bearer ${"r".repeat(64)}`);

    // No welcome within the handshake deadline: try again.
    h.socket().open();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(h.logger.info).toHaveBeenCalledWith(
      expect.objectContaining({ reason: "handshake_timeout" }),
      "[Hub] connection lost",
    );
    await vi.advanceTimersByTimeAsync(1_000);
    expect(h.sockets).toHaveLength(3);
  });

  it("treats failed writes as a lost connection", async () => {
    const h = hubFixture();
    h.link.start();
    await h.flush();
    h.socket().failSends = true;
    h.socket().open();
    expect(h.logger.info).toHaveBeenCalledWith(
      expect.objectContaining({ reason: "send_failed" }),
      "[Hub] connection lost",
    );

    const online = hubFixture();
    const socket = await online.online();
    socket.failSends = true;
    await expect(online.link.verify(1000)).resolves.toBe(false);
    expect(online.link.state).toBe("reconnecting");
  });

  it("drops connections that send text, early or repeated frames", async () => {
    const reasons = async (play: (h: ReturnType<typeof hubFixture>) => Promise<void>) => {
      const h = hubFixture();
      await play(h);
      return h.logger.info.mock.calls
        .filter(([, message]) => message === "[Hub] connection lost")
        .map(([fields]) => (fields as { reason: string }).reason);
    };
    expect(await reasons(async (h) => (await h.online()).receiveText("text"))).toEqual([
      "invalid_frame",
    ]);
    expect(
      await reasons(async (h) => {
        h.link.start();
        await h.flush();
        h.socket().open();
        h.socket().receive({ t: "ack", a: 0 });
      }),
    ).toEqual(["unexpected_frame"]);
    expect(await reasons(async (h) => (await h.online()).welcome())).toEqual(["unexpected_frame"]);
    expect(
      await reasons(async (h) => {
        h.link.start();
        await h.flush();
        h.socket().open();
        h.socket().welcome({ resume_token: undefined });
      }),
    ).toEqual(["invalid_welcome"]);
  });

  it("ignores unknown frame kinds and invalid presence payloads", async () => {
    const h = hubFixture();
    const socket = await h.online();
    socket.receive({ t: "future", seq: 2 });
    socket.receive({ t: "presence", seq: 3 }, "not json");
    expect(h.logger.warn).toHaveBeenCalledWith(expect.anything(), "[Hub] invalid presence ignored");
    expect(h.link.state).toBe("online");
    expect(h.link.serveFor(HOST.host_id)).toBeDefined();
  });

  it("rejects a resumed welcome for another link and starts over with a ticket", async () => {
    const h = hubFixture();
    const socket = await h.online();
    const session = h.link.openSession(HOST.node_id);
    const closed = vi.fn();
    session.onClose(closed);
    socket.drop();
    await h.flush();
    h.socket().open();
    h.socket().welcome({ resumed: true, link: "another-link", resume_token: undefined });
    expect(h.logger.warn).toHaveBeenCalledWith(
      expect.objectContaining({ code: "invalid_resume" }),
      "[Hub] center rejected the link",
    );
    expect(closed).toHaveBeenCalledOnce();
    await h.flush();
    expect(h.ticket).toHaveBeenCalledTimes(2);
  });

  it("forgets a link whose grace period ended before the next attempt", async () => {
    const h = hubFixture();
    h.link.start();
    await h.flush();
    h.socket().open();
    h.socket().welcome({ grace_seconds: 1 });
    h.socket().presence(1, [HOST]);
    // Reconnect attempts keep failing until the grace period is over.
    h.factory.mockImplementation(() => {
      throw new Error("offline");
    });
    h.socket().drop();
    await vi.advanceTimersByTimeAsync(1_000);
    expect(h.link.presence).toBeNull();
    expect(h.link.centerUrl).toBeNull();
    expect(h.link.state).toBe("connecting");
    await vi.advanceTimersByTimeAsync(5_000);
    // Only fresh tickets remain once the link is forgotten.
    expect(h.ticket.mock.calls.length).toBeGreaterThan(1);
  });

  it("acknowledges at once under sustained traffic", async () => {
    const h = hubFixture();
    const socket = await h.online();
    for (let seq = 2; seq <= 40; seq += 1) socket.receive({ t: "open", seq, s: "x", from: "y" });
    expect(socket.frames("ack")).toEqual([{ t: "ack", a: 32 }]);
    await vi.advanceTimersByTimeAsync(50);
    expect(socket.frames("ack")).toEqual([
      { t: "ack", a: 32 },
      { t: "ack", a: 40 },
    ]);
  });

  it("logs through the console by default", async () => {
    const info = vi.spyOn(console, "info").mockImplementation(() => undefined);
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    vi.useFakeTimers();
    const link = new HubLink({
      ticket: () => Promise.reject(new Error("offline")),
      webSocketFactory: () => {
        throw new Error("unused");
      },
    });
    link.start();
    await vi.advanceTimersByTimeAsync(0);
    expect(warn).toHaveBeenCalledWith("[Hub] connection attempt failed", expect.any(String));
    link.stop();
    expect(info).not.toHaveBeenCalled();
  });
});

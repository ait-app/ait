import { afterEach, describe, expect, it, vi } from "vitest";
import { CENTER, HOST, hubFixture, RESUME_TOKEN } from "../../test/hub.js";
import { HUB_FEATURES } from "./frame.js";

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const ALL = [...HUB_FEATURES];

/** A link whose ticket carries the center's HTTP `hub_features`. */
function featureFixture(features: readonly string[] | null = ALL) {
  let issued = 0;
  const ticket = vi.fn(async () => ({
    centerUrl: CENTER,
    ticket: (++issued).toString(16).padStart(64, "0"),
    ...(features ? { features } : {}),
  }));
  const h = hubFixture({ ticket });
  return {
    ...h,
    ticket,
    async connect(welcome: Record<string, unknown> = {}) {
      h.link.start();
      await h.flush();
      const socket = h.socket();
      socket.open();
      socket.welcome({
        f: features ? ALL.filter((f) => features.includes(f)) : undefined,
        ...welcome,
      });
      return socket;
    },
  };
}

function hostsFull(seq: number, ver: number, hosts: object[]) {
  return [{ t: "hosts", seq, ver, full: true }, JSON.stringify({ hosts })] as const;
}

const HUB_HOST = {
  host_id: HOST.host_id,
  node_id: HOST.node_id,
  server_id: "server-1",
  instance_id: "instance-1",
  name: "Workstation",
  hostname: "ws",
  platform: "linux",
  app_version: "1.0",
  state: "online",
  offline_since: null,
  f: ["credit", "download"],
};

describe("hub link features", () => {
  it("sends no features and keeps presence and heartbeat calls for older centers", async () => {
    const h = featureFixture(null);
    const socket = await h.connect({ center_methods: ["session.heartbeat"] });
    expect(socket.frames("hello")).toEqual([{ t: "hello", v: 1, resume: null }]);
    expect(h.link.features).toEqual([]);
    socket.presence(1, [HOST]);
    expect(h.link.ready).toBe(true);
    expect(h.link.serveFor(HOST.host_id)?.node_id).toBe(HOST.node_id);
    void h.link.reportPresence({ appVisible: true, deviceType: "mobile" });
    expect(socket.frames("state")).toEqual([]);
    expect(socket.frames("call")).toEqual([
      expect.objectContaining({ t: "call", method: "session.heartbeat" }),
    ]);
    const session = h.link.openSession(HOST.node_id);
    session.send("hello");
    expect(socket.frames("open")[0]).not.toHaveProperty("w");
    expect(socket.frames("open")[0]).not.toHaveProperty("k");
  });

  it("announces the advertised features it implements and repeats them on every resume", async () => {
    const h = featureFixture(["credit", "lease", "future", "hosts"]);
    const socket = await h.connect();
    expect(socket.frames("hello")).toEqual([
      { t: "hello", v: 1, resume: null, f: ["lease", "hosts", "credit"] },
    ]);
    expect(h.link.features).toEqual(["lease", "hosts", "credit"]);
    expect(h.link.snapshot().features).toEqual(["lease", "hosts", "credit"]);
    socket.drop();
    await h.flush();
    const resumed = h.socket();
    resumed.open();
    expect(resumed.frames("hello")).toEqual([
      { t: "hello", v: 1, resume: { link: "link-1", recv: 0 }, f: ["lease", "hosts", "credit"] },
    ]);
    // A resume that falls back to a fresh ticket still repeats the original set.
    resumed.drop();
    await vi.advanceTimersByTimeAsync(10_000);
    h.socket().drop();
    await vi.advanceTimersByTimeAsync(10_000);
    const ticketed = h.socket();
    ticketed.open();
    expect(ticketed.frames("hello")[0]).toMatchObject({
      resume: { link: "link-1" },
      f: ["lease", "hosts", "credit"],
    });
  });

  it("pings at welcome.ping_seconds", async () => {
    const h = featureFixture();
    const socket = await h.connect({ ping_seconds: 30 });
    await vi.advanceTimersByTimeAsync(29_000);
    expect(socket.frames("ping")).toEqual([]);
    await vi.advanceTimersByTimeAsync(1000);
    expect(socket.frames("ping")).toHaveLength(1);
  });

  it("builds Hosts from hosts frames and ignores deltas after a version gap until a full one", async () => {
    const h = featureFixture();
    const socket = await h.connect();
    expect(h.link.ready).toBe(false);
    // A delta before the first full snapshot is ignored.
    socket.receive({ t: "hosts", seq: 1, ver: 1 }, JSON.stringify({ upsert: [HUB_HOST] }));
    expect(h.link.hosts).toBeNull();
    socket.receive(...hostsFull(2, 2, [HUB_HOST]));
    expect(h.link.ready).toBe(true);
    expect(h.link.serveFor(HOST.host_id)).toMatchObject({ node_id: HOST.node_id, state: "online" });
    expect(h.link.hostFor(HOST.host_id)?.f).toEqual(["credit", "download"]);
    // Presence is not consulted on a hosts link.
    socket.presence(3, []);
    expect(h.link.serveFor(HOST.host_id)).toBeDefined();
    const other = { ...HUB_HOST, host_id: "other", node_id: "other-node", f: [] };
    socket.receive({ t: "hosts", seq: 4, ver: 3 }, JSON.stringify({ upsert: [other] }));
    expect(h.link.hosts?.map((host) => host.host_id)).toEqual([HOST.host_id, "other"]);
    socket.receive(
      { t: "hosts", seq: 5, ver: 4 },
      JSON.stringify({ upsert: [{ ...HUB_HOST, state: "offline", offline_since: "2026-01-01" }] }),
    );
    expect(h.link.serveFor(HOST.host_id)).toBeUndefined();
    expect(h.link.hostFor(HOST.host_id)?.state).toBe("offline");
    // Version 6 skips 5: it and every later delta wait for a full snapshot.
    socket.receive({ t: "hosts", seq: 6, ver: 6 }, JSON.stringify({ remove: ["other"] }));
    socket.receive({ t: "hosts", seq: 7, ver: 7 }, JSON.stringify({ remove: [HOST.host_id] }));
    expect(h.link.hosts?.map((host) => host.host_id)).toEqual([HOST.host_id, "other"]);
    socket.receive(...hostsFull(8, 8, [HUB_HOST]));
    expect(h.link.hosts?.map((host) => host.host_id)).toEqual([HOST.host_id]);
    socket.receive({ t: "hosts", seq: 9, ver: 9 }, JSON.stringify({ remove: [HOST.host_id] }));
    expect(h.link.hosts).toEqual([]);
    expect(h.link.ready).toBe(true);
  });

  it("sends state frames on change and again after a resume", async () => {
    const h = featureFixture();
    const socket = await h.connect({ center_methods: ["session.heartbeat"] });
    await h.link.reportPresence({
      appVisible: true,
      deviceType: "mobile",
      focusedHostId: HOST.host_id,
      focusedAgentId: "agent-1",
      focusedTerminalId: null,
    });
    await h.link.reportPresence({
      appVisible: true,
      deviceType: "mobile",
      focusedHostId: HOST.host_id,
      focusedAgentId: "agent-1",
    });
    expect(socket.frames("state")).toEqual([
      { t: "state", visible: true, focus: { host: HOST.host_id, agent: "agent-1" } },
    ]);
    expect(socket.frames("call")).toEqual([]);
    socket.drop();
    await h.flush();
    // Backgrounding while disconnected is reported once the link resumes.
    await h.link.reportPresence({ appVisible: false, deviceType: "mobile" });
    const resumed = h.socket();
    resumed.open();
    resumed.welcome({ resumed: true, resume_token: undefined, f: ALL });
    expect(resumed.frames("state")).toEqual([{ t: "state", visible: false }]);
  });
});

describe("hub session credit", () => {
  async function creditSession() {
    const h = featureFixture();
    const socket = await h.connect();
    socket.receive(...hostsFull(1, 1, [HUB_HOST]));
    const session = h.link.openSession(HOST.node_id);
    const messages = vi.fn();
    const errors = vi.fn();
    session.onMessage(messages);
    session.onError(errors);
    session.send('{"type":"hello"}');
    return { h, socket, session, messages, errors };
  }

  it("announces its window, waits for peer credit instead of overrunning it, and grants credit", async () => {
    const { socket, session, messages, errors } = await creditSession();
    expect(socket.frames("open")[0]).toMatchObject({ w: 1024 * 1024 });
    // Messages before the accept wait: flow control is decided by the accept.
    session.send(new Uint8Array(10));
    expect(socket.frames("msg")).toEqual([]);
    socket.receive({ t: "accept", seq: 2, s: session.id, w: 65_536 }, "info");
    expect(session.flowControlled).toBe(true);
    expect(socket.frames("msg")).toHaveLength(1);
    session.send(new Uint8Array(65_526));
    session.send(new Uint8Array(100));
    expect(socket.frames("msg")).toHaveLength(2);
    expect(session.bufferedAmount).toBe(100);
    const drained = vi.fn();
    void session.drain().then(drained);
    socket.receive({ t: "credit", s: session.id, c: 99 });
    expect(socket.frames("msg")).toHaveLength(2);
    socket.receive({ t: "credit", s: session.id, c: 100 });
    expect(socket.frames("msg")).toHaveLength(3);
    await Promise.resolve();
    expect(drained).toHaveBeenCalled();
    // Inbound messages grant credit every half of our window.
    socket.receive({ t: "msg", seq: 3, s: session.id, pt: "binary" }, new Uint8Array(400_000));
    expect(socket.frames("credit")).toEqual([]);
    socket.receive({ t: "msg", seq: 4, s: session.id, pt: "binary" }, new Uint8Array(200_000));
    expect(socket.frames("credit")).toEqual([{ t: "credit", s: session.id, c: 600_000 }]);
    expect(messages).toHaveBeenCalledTimes(3);
    expect(errors).not.toHaveBeenCalled();
  });

  it("resends the latest credit after a resume", async () => {
    const { h, socket, session } = await creditSession();
    socket.receive({ t: "accept", seq: 2, s: session.id, w: 65_536 });
    socket.receive({ t: "msg", seq: 3, s: session.id, pt: "binary" }, new Uint8Array(1000));
    socket.drop();
    await h.flush();
    const resumed = h.socket();
    resumed.open();
    resumed.welcome({ resumed: true, resume_token: undefined, recv: 1, f: ALL });
    expect(resumed.frames("credit")).toEqual([{ t: "credit", s: session.id, c: 1000 }]);
  });

  it("keeps uncontrolled sessions when the Host answers without a window", async () => {
    const { socket, session } = await creditSession();
    session.send("queued");
    socket.receive({ t: "accept", seq: 2, s: session.id });
    expect(session.flowControlled).toBe(false);
    expect(socket.frames("msg")).toHaveLength(1);
    session.send(new Uint8Array(200_000));
    expect(socket.frames("msg")).toHaveLength(2);
    socket.receive({ t: "msg", seq: 3, s: session.id, pt: "binary" }, new Uint8Array(900_000));
    expect(socket.frames("credit")).toEqual([]);
  });

  it("waits for link acknowledgements instead of failing a flow-controlled send", async () => {
    const { socket, session, errors } = await creditSession();
    socket.receive({ t: "accept", seq: 2, s: session.id, w: 1024 * 1024 });
    // Keep granting credit so only the 8 MiB link buffer limits sending.
    for (let index = 0; index < 10; index += 1) {
      expect(() => session.send(new Uint8Array(1024 * 1024))).not.toThrow();
      socket.receive({ t: "credit", s: session.id, c: (index + 1) * 1024 * 1024 });
    }
    const sent = socket.frames("msg").length;
    expect(sent).toBeLessThan(10);
    expect(session.bufferedAmount).toBeGreaterThan(0);
    socket.receive({ t: "ack", a: 100 });
    expect(socket.frames("msg")).toHaveLength(10);
    expect(session.bufferedAmount).toBe(0);
    expect(errors).not.toHaveBeenCalled();
  });

  it("still fails an uncontrolled send when the link buffer is full", async () => {
    const h = featureFixture(null);
    const socket = await h.connect();
    socket.presence(1, [HOST]);
    const session = h.link.openSession(HOST.node_id);
    session.send("hello");
    socket.receive({ t: "accept", seq: 2, s: session.id });
    expect(() => {
      for (let index = 0; index < 10; index += 1) session.send(new Uint8Array(1024 * 1024));
    }).toThrow("Hub send buffer is full.");
  });

  it("opens download sessions with the token and grants credit only after consumption", async () => {
    const h = featureFixture();
    const socket = await h.connect();
    socket.receive(...hostsFull(1, 1, [HUB_HOST]));
    const download = h.link.openDownload(HOST.node_id, "token-1");
    expect(socket.frames("open")).toEqual([
      {
        t: "open",
        seq: 1,
        s: download.id,
        to: { node: HOST.node_id },
        k: "download",
        w: 1024 * 1024,
      },
    ]);
    expect(new TextDecoder().decode(socket.sent.at(-1)!.payload)).toBe('{"token":"token-1"}');
    const messages = vi.fn();
    download.onMessage(messages);
    socket.receive({ t: "accept", seq: 2, s: download.id, w: 65_536 });
    expect(messages).not.toHaveBeenCalled();
    socket.receive({ t: "msg", seq: 3, s: download.id, pt: "binary" }, new Uint8Array(600_000));
    expect(socket.frames("credit")).toEqual([]);
    download.consumed(600_000);
    expect(socket.frames("credit")).toEqual([{ t: "credit", s: download.id, c: 600_000 }]);
    download.end("cancelled");
    expect(socket.frames("close")).toEqual([
      { t: "close", seq: 2, s: download.id, code: "cancelled" },
    ]);
  });

  it("refuses download sessions on links without the feature", async () => {
    const h = featureFixture(["lease"]);
    await h.connect();
    expect(() => h.link.openDownload(HOST.node_id, "token")).toThrow("Hub downloads");
  });
});

describe("hub link pause", () => {
  it("drops only the socket and resumes the same link within the grace period", async () => {
    const h = featureFixture();
    const socket = await h.connect();
    const session = h.link.openSession(HOST.node_id);
    session.send("hello");
    socket.receive({ t: "accept", seq: 1, s: session.id, w: 65_536 });
    h.link.pause();
    expect(socket.close).toHaveBeenCalled();
    expect(h.link.state).toBe("reconnecting");
    await vi.advanceTimersByTimeAsync(60_000);
    expect(h.sockets).toHaveLength(1);
    expect(session.closed).toBe(false);
    h.link.start();
    await h.flush();
    const resumed = h.socket();
    expect(resumed.options?.headers?.Authorization).toBe(`Bearer ${RESUME_TOKEN}`);
    resumed.open();
    expect(resumed.frames("hello")[0]).toMatchObject({ resume: { link: "link-1" }, f: ALL });
    expect(h.ticket).toHaveBeenCalledOnce();
  });

  it("starts a new link when the foreground returns after the grace period", async () => {
    const h = featureFixture();
    await h.connect({ grace_seconds: 120 });
    const session = h.link.openSession(HOST.node_id);
    h.link.pause();
    await vi.advanceTimersByTimeAsync(121_000);
    expect(session.closed).toBe(true);
    expect(h.link.state).toBe("unavailable");
    expect(h.sockets).toHaveLength(1);
    h.link.start();
    await h.flush();
    expect(h.ticket).toHaveBeenCalledTimes(2);
    h.socket().open();
    expect(h.socket().frames("hello")[0]).toMatchObject({ resume: null });
  });
});

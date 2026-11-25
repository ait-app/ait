import { afterEach, describe, expect, it, vi } from "vitest";
import { CENTER, HOST, hubFixture, type FakeHubSocket } from "../../test/hub.js";
import { HUB_BULK_FEATURES, HUB_CENTER_FEATURES, HUB_FEATURES } from "./frame.js";
import type { HubTicket } from "./link.js";

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const HOME_HUB = "wss://center.test/api/v1/hub";
const JP = "https://hub.jp.test/api";
const JP_HUB = "wss://hub.jp.test/api/v1/hub";
const EU = "https://hub.eu.test/api";
const EU_HUB = "wss://hub.eu.test/api/v1/hub";
const ALL = [...HUB_FEATURES];
const REDIRECTING = [...HUB_FEATURES, "redirect"];
const WINDOW = 1024 * 1024;

/** A Host the center lists as served by the `jp-dp` DP. */
const ELSEWHERE = {
  ...HOST,
  host_id: "22222222-2222-4222-8222-222222222222",
  node_id: "jp-node",
  name: "Tokyo",
  dp: "jp-dp",
  hub_url: JP_HUB,
  center: JP,
};

/** An account link whose center advertises `features` with every ticket. */
function redirectFixture(
  features: readonly string[] = [...HUB_CENTER_FEATURES, "redirect"],
  centerUrl = CENTER,
) {
  let issued = 0;
  const ticket = vi.fn(
    async (): Promise<HubTicket> => ({
      centerUrl,
      ticket: (++issued).toString(16).padStart(64, "0"),
      features,
    }),
  );
  const h = hubFixture({ ticket });
  return {
    ...h,
    ticket,
    /** Start the account link and complete its handshake with one full `hosts` snapshot. */
    async start(hosts: object[] = [HOST, ELSEWHERE]) {
      h.link.start();
      await h.flush();
      const socket = h.socket();
      socket.open();
      socket.welcome({ ping_seconds: 3600 });
      socket.hosts(1, hosts);
      return socket;
    },
    /** Complete the handshake of the link the last session to another DP created. */
    async remote(url = JP_HUB, link = "jp-link") {
      await h.flush();
      const socket = h.socket();
      expect(socket.url).toBe(url);
      socket.open();
      socket.welcome({ link, ping_seconds: 3600 });
      return socket;
    },
  };
}

function payload(socket: FakeHubSocket, t: string, index = 0): string {
  return new TextDecoder().decode(socket.sent.filter((f) => f.header.t === t)[index].payload);
}

function redirect(node: string, center: string, hubUrl: string, epoch: number) {
  return { node, hub_url: hubUrl, center, epoch };
}

describe("hub redirect negotiation", () => {
  it("announces redirect only when the center lists it in hub_features", async () => {
    const h = redirectFixture([...HUB_CENTER_FEATURES]);
    const socket = await h.start([HOST]);
    expect(socket.frames("hello")).toEqual([{ t: "hello", v: 1, resume: null, f: ALL }]);
    expect(h.link.redirects).toBe(false);
    h.link.stop();
  });

  it("enables redirect from welcome.f and stays online on a DP that leaves it out", async () => {
    const h = redirectFixture();
    h.link.start();
    await h.flush();
    const socket = h.socket();
    socket.open();
    expect(socket.frames("hello")).toEqual([{ t: "hello", v: 1, resume: null, f: REDIRECTING }]);
    // A DP whose redirect switch is off masks it; it is optional, unlike the six required ones.
    socket.welcome({ f: ALL, ping_seconds: 3600 });
    socket.hosts(1, [HOST]);
    expect(h.link.state).toBe("online");
    expect(h.link.redirects).toBe(false);
    h.link.stop();

    const enabled = redirectFixture();
    await enabled.start([HOST]);
    expect(enabled.link.redirects).toBe(true);
    enabled.link.stop();
  });

  it("repeats the announced set on every resume, even when later tickets drop redirect", async () => {
    const h = redirectFixture();
    const socket = await h.start([HOST]);
    socket.drop();
    await h.flush();
    const resumed = h.socket();
    resumed.open();
    expect(resumed.frames("hello")).toEqual([
      { t: "hello", v: 1, resume: { link: "link-1", recv: 1 }, f: REDIRECTING },
    ]);
    h.ticket.mockImplementation(async () => ({
      centerUrl: CENTER,
      ticket: "f".repeat(64),
      features: [...HUB_CENTER_FEATURES],
    }));
    // Two failed resume-token connections fall back to a ticket; the link keeps its set.
    resumed.drop();
    await vi.advanceTimersByTimeAsync(10_000); // virtual time
    h.socket().drop();
    await vi.advanceTimersByTimeAsync(10_000); // virtual time
    const ticketed = h.socket();
    expect(ticketed.options?.headers?.Authorization).toBe(`Bearer ${"f".repeat(64)}`);
    ticketed.open();
    expect(ticketed.frames("hello")[0]).toMatchObject({
      resume: { link: "link-1" },
      f: REDIRECTING,
    });
    h.link.stop();
  });

  it("never announces redirect on a bulk link", async () => {
    const h = redirectFixture();
    await h.start([HOST]);
    const bulk = h.link.bulkLink(async () => ({
      centerUrl: CENTER,
      ticket: "b".repeat(64),
      features: [...HUB_CENTER_FEATURES, "redirect"],
    }));
    bulk.start();
    await h.flush();
    const socket = h.socket();
    socket.open();
    expect(socket.frames("hello")).toEqual([
      { t: "hello", v: 1, resume: null, f: [...HUB_BULK_FEATURES] },
    ]);
    bulk.stop();
    h.link.stop();
  });

  it("keeps where another DP serves a Host and drops invalid locations", async () => {
    const h = redirectFixture();
    const insecure = {
      ...HOST,
      host_id: "33333333-3333-4333-8333-333333333333",
      node_id: "insecure-node",
      dp: "plain",
      hub_url: "ws://plain.test/v1/hub",
      center: "http://plain.test",
    };
    const socket = await h.start([HOST, ELSEWHERE, insecure]);
    expect(h.link.hostFor(ELSEWHERE.host_id)).toMatchObject({
      state: "online",
      node_id: "jp-node",
      dp: "jp-dp",
      hub_url: JP_HUB,
      center: JP,
    });
    expect(h.link.hostFor(HOST.host_id)).not.toHaveProperty("center");
    expect(h.link.hostFor(insecure.host_id)).not.toHaveProperty("hub_url");
    expect(h.link.hostFor(insecure.host_id)).not.toHaveProperty("dp");
    // Upserts replace whole entries: a Host that moved to this DP loses its location.
    const { dp: _dp, hub_url: _hub, center: _center, ...here } = ELSEWHERE;
    socket.receive({ t: "hosts", seq: 2, ver: 2 }, JSON.stringify({ upsert: [here] }));
    expect(h.link.hostFor(ELSEWHERE.host_id)).not.toHaveProperty("center");
    h.link.stop();
  });
});

describe("hub links to other DPs", () => {
  it("opens sessions to a Host another DP serves over a second link to that DP", async () => {
    const h = redirectFixture();
    const home = await h.start();
    expect(home.url).toBe(HOME_HUB);
    const session = h.link.openSession("jp-node");
    const messages: unknown[] = [];
    session.onMessage((data) => messages.push(data));
    session.send("hello");
    session.send("early");
    const remote = await h.remote();
    // The ticket comes from the account's center and is used on the other DP.
    expect(h.ticket).toHaveBeenCalledTimes(2);
    expect(remote.options?.headers?.Authorization).toBe(`Bearer ${"2".padStart(64, "0")}`);
    expect(remote.frames("hello")).toEqual([{ t: "hello", v: 1, resume: null, f: REDIRECTING }]);
    expect(remote.frames("open")).toEqual([
      { t: "open", seq: 1, s: session.id, to: { node: "jp-node" }, w: WINDOW },
    ]);
    expect(payload(remote, "open")).toBe("hello");
    expect(home.frames("open")).toEqual([]);
    remote.receive({ t: "accept", seq: 1, s: session.id, w: 65_536 }, "server-info");
    expect(messages).toEqual(["server-info"]);
    expect(remote.frames("msg")).toEqual([{ t: "msg", seq: 2, s: session.id, pt: "text" }]);

    // Hosts of the account link's DP stay on it, and that DP's link is shared.
    const local = h.link.openSession(HOST.node_id);
    local.send("hello");
    expect(home.frames("open")).toEqual([
      { t: "open", seq: 1, s: local.id, to: { node: HOST.node_id }, w: WINDOW },
    ]);
    const again = h.link.openSession("jp-node");
    again.send("hello");
    expect(h.sockets).toHaveLength(2);
    expect(remote.frames("open")).toHaveLength(2);
    // The account link reports presence; the other link never does.
    await h.link.reportPresence({ appVisible: true, deviceType: "phone", focusedHostId: "x" });
    expect(home.frames("state")).toHaveLength(1);
    expect(remote.frames("state")).toEqual([]);
    h.link.stop();
  });

  it("closes a link to another DP a minute after its last session and reopens it on demand", async () => {
    const h = redirectFixture();
    const home = await h.start();
    const session = h.link.openSession("jp-node");
    session.send("hello");
    const remote = await h.remote();
    remote.receive({ t: "accept", seq: 1, s: session.id, w: 65_536 });
    session.close();
    await vi.advanceTimersByTimeAsync(59_000); // virtual time
    expect(remote.close).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1000); // virtual time
    expect(remote.close).toHaveBeenCalled();
    expect(home.close).not.toHaveBeenCalled();
    expect(h.link.state).toBe("online");
    const next = h.link.openSession("jp-node");
    next.send("hello");
    const reopened = await h.remote(JP_HUB, "jp-link-2");
    expect(reopened).not.toBe(remote);
    expect(reopened.frames("hello")[0]).toMatchObject({ resume: null });
    expect(reopened.frames("open")).toEqual([expect.objectContaining({ s: next.id })]);
    h.link.stop();
  });

  it("pauses, resumes and stops links to other DPs with the account link", async () => {
    const h = redirectFixture();
    const home = await h.start();
    const session = h.link.openSession("jp-node");
    const closed = vi.fn();
    session.onClose(closed);
    session.send("hello");
    const remote = await h.remote();
    remote.receive({ t: "accept", seq: 1, s: session.id, w: 65_536 });

    h.link.pause();
    expect(remote.close).toHaveBeenCalled();
    expect(home.close).toHaveBeenCalled();
    const sockets = h.sockets.length;
    h.link.start();
    await h.flush();
    const resumed = h.sockets.slice(sockets);
    expect(resumed.map((socket) => socket.url).sort()).toEqual([HOME_HUB, JP_HUB].sort());
    const jp = resumed.find((socket) => socket.url === JP_HUB)!;
    jp.open();
    expect(jp.frames("hello")[0]).toMatchObject({ resume: { link: "jp-link" } });
    expect(closed).not.toHaveBeenCalled();

    h.link.stop();
    expect(closed).toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(30_000); // virtual time
    expect(h.sockets).toHaveLength(sockets + 2);
  });

  it("fails sessions waiting on a link to another DP that cannot connect", async () => {
    const h = redirectFixture();
    await h.start();
    const session = h.link.openSession("jp-node");
    const closed = vi.fn();
    session.onClose(closed);
    session.send("hello");
    await h.flush();
    const remote = h.socket();
    expect(remote.url).toBe(JP_HUB);
    remote.fail();
    expect(closed).toHaveBeenCalledWith(expect.objectContaining({ code: 1006 }));
    expect(session.closed).toBe(true);
    // Its open is dropped with it: a later connection of that link does not send it.
    await vi.advanceTimersByTimeAsync(300); // virtual time
    const retry = h.socket();
    expect(retry).not.toBe(remote);
    retry.open();
    retry.welcome({ ping_seconds: 3600 });
    expect(retry.frames("open")).toEqual([]);
    h.link.stop();
  });

  it("keeps every session on the account link while the center does not enable redirect", async () => {
    const h = redirectFixture([...HUB_CENTER_FEATURES]);
    const home = await h.start();
    const session = h.link.openSession("jp-node");
    session.send("hello");
    await h.flush();
    expect(h.sockets).toHaveLength(1);
    expect(home.frames("open")).toEqual([
      { t: "open", seq: 1, s: session.id, to: { node: "jp-node" }, w: WINDOW },
    ]);
    h.link.stop();
  });
});

describe("remote Hub link lifecycle", () => {
  it("drops cancelled opens before the first welcome without leaving sequence gaps", async () => {
    const h = redirectFixture();
    await h.start();
    const cancelled = h.link.openSession("jp-node", { kind: "download" });
    cancelled.send("download request");
    const abandoned = h.link.openSession("jp-node");
    abandoned.send("abandoned hello");
    const active = h.link.openSession("jp-node");
    active.send("live hello");
    cancelled.end("cancelled");
    abandoned.abort();

    const remote = await h.remote();
    expect(remote.frames("open")).toEqual([
      { t: "open", seq: 1, s: active.id, to: { node: "jp-node" }, w: WINDOW },
    ]);
    expect(payload(remote, "open")).toBe("live hello");
    expect(remote.frames("close")).toEqual([]);
    h.link.stop();
  });

  it("drops a timed-out open when the first handshake resumes after a pause", async () => {
    const h = redirectFixture();
    await h.start();
    const session = h.link.openSession("jp-node");
    session.send("hello");
    await h.flush();
    h.link.pause();
    await vi.advanceTimersByTimeAsync(30_000); // virtual time
    expect(session.closed).toBe(true);

    h.link.start();
    await h.flush();
    const remote = h.sockets.filter((socket) => socket.url === JP_HUB).at(-1)!;
    remote.open();
    remote.welcome({ link: "jp-link" });
    expect(remote.frames("open")).toEqual([]);
    expect(remote.frames("close")).toEqual([]);
    h.link.stop();
  });

  it("verifies home and remote sockets independently when the app returns to the foreground", async () => {
    const h = redirectFixture();
    const home = await h.start();
    const session = h.link.openSession("jp-node");
    session.send("hello");
    const remote = await h.remote();
    remote.receive({ t: "accept", seq: 1, s: session.id, w: WINDOW });

    const verified = h.link.verify(1000);
    const [ping] = home.frames("ping");
    expect(remote.frames("ping")).toHaveLength(1);
    home.receive({ t: "pong", n: ping.n });
    await expect(verified).resolves.toBe(true);
    await vi.advanceTimersByTimeAsync(1000); // virtual time
    expect(home.close).not.toHaveBeenCalled();
    expect(remote.close).toHaveBeenCalledOnce();
    await vi.advanceTimersByTimeAsync(1); // virtual time
    const resumed = h.socket();
    expect(resumed).not.toBe(remote);
    expect(resumed.url).toBe(JP_HUB);
    resumed.open();
    expect(resumed.frames("hello")[0]).toMatchObject({ resume: { link: "jp-link", recv: 1 } });
    resumed.welcome({ link: "jp-link", resumed: true, recv: 1 });
    expect(session.closed).toBe(false);
    h.link.stop();
  });
});

describe("hub session redirects", () => {
  it("repeats a refused open on the DP the center names, with messages after the accept", async () => {
    const h = redirectFixture();
    // This DP still lists the Host as its own, e.g. before the move reached it.
    const home = await h.start([HOST]);
    const session = h.link.openSession(HOST.node_id);
    const closed = vi.fn();
    session.onClose(closed);
    session.send("hello");
    session.send("early");
    home.receive({
      t: "close",
      seq: 2,
      s: session.id,
      code: "redirect",
      redirect: redirect(HOST.node_id, JP, JP_HUB, 7),
    });
    const remote = await h.remote();
    expect(remote.frames("open")).toEqual([
      { t: "open", seq: 1, s: session.id, to: { node: HOST.node_id }, w: WINDOW },
    ]);
    expect(payload(remote, "open")).toBe("hello");
    // The refusing DP already ended the session; nothing more goes there for it.
    expect(home.frames("close")).toEqual([]);
    expect(home.frames("msg")).toEqual([]);
    expect(remote.frames("msg")).toEqual([]);
    remote.receive({ t: "accept", seq: 1, s: session.id, w: 65_536 });
    expect(remote.frames("msg")).toEqual([{ t: "msg", seq: 2, s: session.id, pt: "text" }]);
    expect(payload(remote, "msg")).toBe("early");
    expect(closed).not.toHaveBeenCalled();
    expect(session.closeCode).toBeNull();
    h.link.stop();
  });

  it("returns a session another DP redirects to the account link's own DP", async () => {
    const h = redirectFixture();
    const home = await h.start();
    const session = h.link.openSession("jp-node");
    session.send("hello");
    const remote = await h.remote();
    remote.receive({
      t: "close",
      seq: 1,
      s: session.id,
      code: "redirect",
      redirect: redirect("jp-node", CENTER, HOME_HUB, 9),
    });
    expect(h.sockets).toHaveLength(2);
    expect(home.frames("open")).toEqual([
      { t: "open", seq: 1, s: session.id, to: { node: "jp-node" }, w: WINDOW },
    ]);
    expect(payload(home, "open")).toBe("hello");
    home.receive({ t: "accept", seq: 2, s: session.id, w: 65_536 });
    // Without sessions the other link closes after its idle minute.
    await vi.advanceTimersByTimeAsync(60_000); // virtual time
    expect(remote.close).toHaveBeenCalled();
    expect(session.closed).toBe(false);
    h.link.stop();
  });

  it("uses the original home link while its logical identity is being rebuilt", async () => {
    const h = redirectFixture();
    const home = await h.start();
    const session = h.link.openSession("jp-node");
    session.send("hello");
    const remote = await h.remote();
    home.drop();
    await h.flush();
    const resume = h.socket();
    resume.open();
    resume.receive({ t: "error", code: "resume_expired" });
    expect(h.link.centerUrl).toBeNull();
    remote.receive({
      t: "close",
      seq: 1,
      s: session.id,
      code: "redirect",
      redirect: redirect("jp-node", CENTER, HOME_HUB, 9),
    });
    await h.flush();
    expect(h.sockets.map((socket) => socket.url)).toEqual([HOME_HUB, JP_HUB, HOME_HUB, HOME_HUB]);
    const fresh = h.socket();
    fresh.open();
    fresh.welcome({ link: "new-home" });
    expect(fresh.frames("open")).toEqual([
      { t: "open", seq: 1, s: session.id, to: { node: "jp-node" }, w: WINDOW },
    ]);
    expect(session.closed).toBe(false);
    h.link.stop();
  });

  it("matches the home DP advertised with a different loopback hostname", async () => {
    const h = redirectFixture(REDIRECTING, "http://localhost:18080/api");
    const host = {
      ...ELSEWHERE,
      center: "http://127.0.0.1:18081/api",
      hub_url: "ws://127.0.0.1:18081/api/v1/hub",
    };
    const home = await h.start([host]);
    const session = h.link.openSession(host.node_id);
    session.send("hello");
    const remote = await h.remote(host.hub_url);
    remote.receive({
      t: "close",
      seq: 1,
      s: session.id,
      code: "redirect",
      redirect: redirect(
        host.node_id,
        "http://127.0.0.1:18080/api",
        "ws://127.0.0.1:18080/api/v1/hub",
        9,
      ),
    });
    await h.flush();
    expect(h.sockets).toHaveLength(2);
    expect(home.frames("open")).toEqual([
      { t: "open", seq: 1, s: session.id, to: { node: host.node_id }, w: WINDOW },
    ]);
    h.link.stop();
  });

  it("reports a transport failure when the DP named by a redirect cannot connect", async () => {
    const h = redirectFixture();
    const home = await h.start([HOST]);
    const session = h.link.openSession(HOST.node_id);
    const closed = vi.fn();
    session.onClose(closed);
    session.send("hello");
    home.receive({
      t: "close",
      seq: 2,
      s: session.id,
      code: "redirect",
      redirect: redirect(HOST.node_id, JP, JP_HUB, 7),
    });
    await h.flush();
    h.socket().fail();
    expect(session.closeCode).toBeNull();
    expect(closed).toHaveBeenCalledExactlyOnceWith({
      code: 1006,
      reason: "Hub link reset (unreachable).",
    });
    h.link.stop();
  });

  it.each([
    ["names the DP that refused it", [redirect(HOST.node_id, CENTER, HOME_HUB, 3)]],
    ["has an insecure Hub URL", [redirect(HOST.node_id, JP, "ws://hub.jp.test/api/v1/hub", 3)]],
    ["names another node", [redirect("another-node", JP, JP_HUB, 3)]],
    [
      "is older than a redirect already followed",
      [redirect(HOST.node_id, JP, JP_HUB, 7), redirect(HOST.node_id, EU, EU_HUB, 5)],
    ],
    [
      "would be the fourth for one open",
      [
        redirect(HOST.node_id, JP, JP_HUB, 1),
        redirect(HOST.node_id, EU, EU_HUB, 2),
        redirect(HOST.node_id, JP, JP_HUB, 3),
        redirect(HOST.node_id, EU, EU_HUB, 4),
      ],
    ],
  ])("fails with redirect when the redirect %s", async (_case, redirects) => {
    const h = redirectFixture();
    const home = await h.start([HOST]);
    const session = h.link.openSession(HOST.node_id);
    const closed = vi.fn();
    session.onClose(closed);
    session.send("hello");
    const links = new Map<string, FakeHubSocket>([[HOME_HUB, home]]);
    let current = home;
    for (const [index, target] of redirects.entries()) {
      const seq = 100 * (index + 1);
      current.receive({ t: "close", seq, s: session.id, code: "redirect", redirect: target });
      if (index === redirects.length - 1) break;
      await h.flush();
      const next = links.get(target.hub_url) ?? h.socket();
      if (!links.has(target.hub_url)) {
        next.open();
        next.welcome({ link: `link-${target.hub_url}`, ping_seconds: 3600 });
        links.set(target.hub_url, next);
      }
      expect(next.frames("open").at(-1)).toMatchObject({ s: session.id });
      current = next;
    }
    expect(session.closeCode).toBe("redirect");
    expect(closed).toHaveBeenCalledWith(expect.objectContaining({ hubCode: "redirect" }));
    h.link.stop();
  });

  it("treats a redirect close after the accept as an ordinary close", async () => {
    const h = redirectFixture();
    const home = await h.start([HOST]);
    const session = h.link.openSession(HOST.node_id);
    session.send("hello");
    home.receive({ t: "accept", seq: 2, s: session.id, w: 65_536 });
    home.receive({
      t: "close",
      seq: 3,
      s: session.id,
      code: "redirect",
      redirect: redirect(HOST.node_id, JP, JP_HUB, 3),
    });
    await h.flush();
    expect(session.closeCode).toBe("redirect");
    expect(h.sockets).toHaveLength(1);
    h.link.stop();
  });
});

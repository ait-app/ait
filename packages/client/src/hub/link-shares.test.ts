import { afterEach, describe, expect, it, vi } from "vitest";
import { type FakeHubSocket, hubFixture } from "../../test/hub.js";

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const METHODS = ["share.subscribe", "org.members.list", "share.revoke"];

const ORG = {
  organizationId: "org-1",
  name: "Acme",
  slug: "acme",
  status: "active",
  myRole: "admin",
  memberVersion: 3,
};

const SHARE = {
  shareId: "share-1",
  organizationId: "org-1",
  organizationName: "Acme",
  hostId: "host-1",
  nodeId: "alice-node",
  hostName: "alice-mbp",
  owner: { userId: "alice", displayName: "Alice" },
  workspaceKey: "ws_1f2e",
  displayName: "api-server",
  projectName: "Acme API",
  permission: "operate",
  state: "online",
};

async function online(methods: string[] = METHODS) {
  const h = hubFixture();
  h.link.start();
  await h.flush();
  const socket = h.socket();
  socket.open();
  socket.welcome({ center_methods: methods });
  return { h, socket };
}

function orgs(socket: FakeHubSocket, seq: number, items: object[]) {
  socket.receive({ t: "orgs", seq }, JSON.stringify({ orgs: items }));
}

function shares(socket: FakeHubSocket, seq: number, items: object[]) {
  socket.receive({ t: "share_presence", seq }, JSON.stringify({ shares: items }));
}

/** Reply to the latest `share.subscribe` call on `socket`. */
function accept(socket: FakeHubSocket, seq: number, result: object = { result: {} }) {
  const call = socket.frames("call").at(-1)!;
  expect(call.method).toBe("share.subscribe");
  socket.receive({ t: "reply", seq, id: call.id }, JSON.stringify(result));
}

describe("hub link organization and share snapshots", () => {
  it("subscribes once and tracks sequenced snapshots", async () => {
    const { h, socket } = await online();
    expect(h.link.sharesSupported).toBe(true);
    expect(h.link.orgs).toBeNull();
    expect(h.link.shares).toBeNull();
    const changed = vi.fn();
    h.link.subscribe(changed);
    const stop = h.link.subscribeShares();
    h.link.subscribeShares();
    expect(socket.frames("call")).toMatchObject([{ t: "call", seq: 1, method: "share.subscribe" }]);
    expect(new TextDecoder().decode(socket.sent.at(-1)!.payload)).toBe("{}");
    accept(socket, 1);
    orgs(socket, 2, [ORG, { organizationId: "broken" }]);
    shares(socket, 3, [SHARE, { ...SHARE, shareId: "share-2", state: "gone" }]);
    expect(h.link.orgs).toEqual([ORG]);
    expect(h.link.shares).toEqual([SHARE]);
    expect(changed).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(50);
    expect(socket.frames("ack").at(-1)).toEqual({ t: "ack", a: 3 });

    // Replays after a resume are dropped; newer snapshots replace older ones.
    shares(socket, 3, []);
    expect(h.link.shares).toEqual([SHARE]);
    shares(socket, 4, [{ ...SHARE, state: "reconnecting" }]);
    expect(h.link.shares).toEqual([{ ...SHARE, state: "reconnecting" }]);
    expect(changed).toHaveBeenCalledTimes(3);
    // An identical snapshot does not notify.
    orgs(socket, 5, [ORG]);
    expect(changed).toHaveBeenCalledTimes(3);
    // A malformed snapshot is ignored but still consumes its sequence number.
    socket.receive({ t: "orgs", seq: 6 }, "{}");
    expect(h.link.orgs).toEqual([ORG]);
    await vi.advanceTimersByTimeAsync(50);
    expect(socket.frames("ack").at(-1)).toEqual({ t: "ack", a: 6 });
    expect(h.logger.warn).toHaveBeenCalledWith(
      expect.objectContaining({ kind: "orgs" }),
      "[Hub] invalid snapshot ignored",
    );
    stop();
    stop();
    h.link.stop();
  });

  it("keeps the subscription across a resume and renews it on a fresh link", async () => {
    const { h, socket } = await online();
    h.link.subscribeShares();
    accept(socket, 1);
    orgs(socket, 2, [ORG]);
    shares(socket, 3, [SHARE]);
    await vi.advanceTimersByTimeAsync(50);

    socket.drop();
    await h.flush();
    const resumed = h.socket();
    resumed.open();
    resumed.welcome({ resumed: true, recv: 1, center_methods: METHODS });
    expect(resumed.frames("call")).toEqual([]);
    expect(h.link.orgs).toEqual([ORG]);

    resumed.drop();
    await h.flush();
    const fresh = h.socket();
    fresh.open();
    expect(h.link.shares).toEqual([SHARE]);
    fresh.welcome({ link: "link-2", center_methods: METHODS });
    expect(h.link.orgs).toBeNull();
    expect(h.link.shares).toBeNull();
    expect(fresh.frames("call")).toMatchObject([{ seq: 1, method: "share.subscribe" }]);
    accept(fresh, 1);
    shares(fresh, 2, []);
    expect(h.link.shares).toEqual([]);
    h.link.stop();
  });

  it("retries a rejected subscription and stops renewing once disposed", async () => {
    const { h, socket } = await online();
    const stop = h.link.subscribeShares();
    accept(socket, 1, { error: { code: "internal", message: "busy" } });
    await h.flush();
    expect(socket.frames("call")).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(3000);
    expect(socket.frames("call")).toHaveLength(2);
    accept(socket, 2);
    await h.flush();
    stop();

    socket.drop();
    await h.flush();
    const fresh = h.socket();
    fresh.open();
    fresh.welcome({ link: "link-2", center_methods: METHODS });
    expect(fresh.frames("call")).toEqual([]);
    h.link.stop();
  });

  it("does not subscribe on centers without shares", async () => {
    const { h, socket } = await online(["push.register"]);
    expect(h.link.sharesSupported).toBe(false);
    h.link.subscribeShares();
    await h.flush();
    expect(socket.frames("call")).toEqual([]);
    await expect(h.link.call("share.subscribe", {})).rejects.toMatchObject({
      code: "unsupported",
    });
    h.link.stop();
  });
});

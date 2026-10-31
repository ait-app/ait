import { afterEach, describe, expect, it, vi } from "vitest";
import { type FakeHubSocket, hubFixture } from "../../test/hub.js";

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const METHODS = ["share.subscribe", "group.members.list", "share.revoke"];

const GROUP = {
  groupId: "group-1",
  name: "Acme",
  slug: "acme",
  status: "active",
  myRole: "admin",
  memberVersion: 3,
};

const SHARE = {
  shareId: "share-1",
  groupId: "group-1",
  groupName: "Acme",
  hostId: "host-1",
  nodeId: "alice-node",
  hostName: "alice-mbp",
  owner: { userId: "alice", displayName: "Alice" },
  resourceKind: "workspace",
  resourceKey: "ws_1f2e",
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

function groups(socket: FakeHubSocket, seq: number, items: object[]) {
  socket.receive({ t: "groups", seq }, JSON.stringify({ groups: items }));
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

describe("hub link group and share snapshots", () => {
  it("subscribes once and tracks sequenced snapshots", async () => {
    const { h, socket } = await online();
    expect(h.link.sharesSupported).toBe(true);
    expect(h.link.groups).toBeNull();
    expect(h.link.shares).toBeNull();
    const changed = vi.fn();
    h.link.subscribe(changed);
    const stop = h.link.subscribeShares();
    h.link.subscribeShares();
    expect(socket.frames("call")).toMatchObject([{ t: "call", seq: 1, method: "share.subscribe" }]);
    expect(new TextDecoder().decode(socket.sent.at(-1)!.payload)).toBe("{}");
    accept(socket, 1);
    const PROJECT = { ...SHARE, shareId: "share-3", resourceKind: "project", resourceKey: "prj_1" };
    const NODE = { ...SHARE, shareId: "share-4", resourceKind: "node", resourceKey: "" };
    groups(socket, 2, [
      GROUP,
      { groupId: "broken" },
      { ...GROUP, organizationId: "x", groupId: 1 },
    ]);
    shares(socket, 3, [
      SHARE,
      PROJECT,
      NODE,
      { ...SHARE, shareId: "share-2", state: "gone" },
      { ...SHARE, shareId: "share-5", resourceKind: "organization" },
      { ...SHARE, shareId: "share-6", resourceKey: undefined },
      { ...SHARE, shareId: "share-7", owner: { userId: "alice", displayName: 7 } },
    ]);
    expect(h.link.groups).toEqual([GROUP]);
    expect(h.link.shares).toEqual([SHARE, PROJECT, NODE]);
    expect(changed).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(50);
    expect(socket.frames("ack").at(-1)).toEqual({ t: "ack", a: 3 });

    // Replays after a resume are dropped; newer snapshots replace older ones.
    shares(socket, 3, []);
    expect(h.link.shares).toEqual([SHARE, PROJECT, NODE]);
    shares(socket, 4, [{ ...SHARE, state: "reconnecting" }]);
    expect(h.link.shares).toEqual([{ ...SHARE, state: "reconnecting" }]);
    expect(changed).toHaveBeenCalledTimes(3);
    // Owners without a display name, and with a username, are kept.
    const NAMELESS = { ...SHARE, owner: { userId: "alice", displayName: null, username: "alice" } };
    shares(socket, 5, [NAMELESS]);
    expect(h.link.shares).toEqual([NAMELESS]);
    shares(socket, 6, [{ ...SHARE, state: "reconnecting" }]);
    expect(changed).toHaveBeenCalledTimes(5);
    // An identical snapshot does not notify.
    groups(socket, 7, [GROUP]);
    expect(changed).toHaveBeenCalledTimes(5);
    // A malformed snapshot is ignored but still consumes its sequence number.
    socket.receive({ t: "groups", seq: 8 }, "{}");
    expect(h.link.groups).toEqual([GROUP]);
    await vi.advanceTimersByTimeAsync(50);
    expect(socket.frames("ack").at(-1)).toEqual({ t: "ack", a: 8 });
    expect(h.logger.warn).toHaveBeenCalledWith(
      expect.objectContaining({ kind: "groups" }),
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
    groups(socket, 2, [GROUP]);
    shares(socket, 3, [SHARE]);
    await vi.advanceTimersByTimeAsync(50);

    socket.drop();
    await h.flush();
    const resumed = h.socket();
    resumed.open();
    resumed.welcome({ resumed: true, recv: 1, center_methods: METHODS });
    expect(resumed.frames("call")).toEqual([]);
    expect(h.link.groups).toEqual([GROUP]);

    resumed.drop();
    await h.flush();
    const fresh = h.socket();
    fresh.open();
    expect(h.link.shares).toEqual([SHARE]);
    fresh.welcome({ link: "link-2", center_methods: METHODS });
    expect(h.link.groups).toBeNull();
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

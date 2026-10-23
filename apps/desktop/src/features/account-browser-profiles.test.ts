import { EventEmitter } from "node:events";
import { beforeEach, expect, test, vi } from "vitest";
import type { WebContents } from "electron";
import type { AccountSessionManager } from "../daemon/account-session.js";
import { AccountBrowserProfiles } from "./account-browser-profiles.js";

const mocks = vi.hoisted(() => ({
  sessions: new Map<string, { setProxy: ReturnType<typeof vi.fn> }>(),
  close: vi.fn(),
  closeAll: vi.fn(),
  listen: vi.fn(),
  daemonTransport: vi.fn(),
}));
vi.mock("electron", () => ({
  app: { getPath: () => "/not-a-browser-profile-directory" },
  session: {
    fromPartition: (partition: string) => {
      let value = mocks.sessions.get(partition);
      if (!value) {
        value = { setProxy: vi.fn().mockResolvedValue(undefined) };
        mocks.sessions.set(partition, value);
      }
      return value;
    },
  },
}));
vi.mock("../daemon/account-transport.js", () => ({
  AccountTransportManager: class {
    closeAll = mocks.closeAll;
  },
}));
vi.mock("./browser-daemon-transport.js", () => ({
  BrowserDaemonTransport: class {
    constructor(input: unknown) {
      mocks.daemonTransport(input);
    }
  },
}));
vi.mock("./browser-network.js", () => ({
  BrowserRelayNetwork: class {
    close = mocks.close;
    listen = mocks.listen;
  },
  browserProxyConfig: (port: number) => ({ proxyRules: `socks5://127.0.0.1:${port}` }),
}));

const input = {
  hostId: "00000000-0000-4000-8000-000000000001",
  serverId: "00000000-0000-4000-8000-000000000002",
  center: "https://center.test",
};
const owner = (id: number) => Object.assign(new EventEmitter(), { id }) as unknown as WebContents;
const account = {} as AccountSessionManager;
beforeEach(() => {
  vi.clearAllMocks();
  mocks.sessions.clear();
  mocks.listen.mockResolvedValue(12345);
});

test("shares a Host profile across windows and closes its route only after the last owner", async () => {
  const profiles = new AccountBrowserProfiles(account);
  const first = owner(1);
  const second = owner(2);
  const a = await profiles.prepare(first, input);
  const b = await profiles.prepare(second, {
    center: input.center,
    serverId: input.serverId,
    hostId: input.hostId,
  });
  expect(a).toEqual(b);
  expect(mocks.listen).toHaveBeenCalledTimes(1);
  expect(profiles.partitions(1)).toEqual([a.profilePartition]);
  expect(profiles.partitions(3)).toEqual([]);
  first.emit("destroyed");
  expect(mocks.close).not.toHaveBeenCalled();
  expect(profiles.partitions(1)).toEqual([]);
  second.emit("destroyed");
  expect(mocks.close).toHaveBeenCalledTimes(1);
});

test("isolates different Hosts and applies routing before resolving the profile", async () => {
  const profiles = new AccountBrowserProfiles(account);
  const first = await profiles.prepare(owner(1), input);
  const second = await profiles.prepare(owner(2), {
    ...input,
    hostId: "00000000-0000-4000-8000-000000000003",
  });
  expect(first.profilePartition).not.toBe(second.profilePartition);
  expect(mocks.sessions.get(first.profilePartition)?.setProxy).toHaveBeenCalledWith({
    proxyRules: "socks5://127.0.0.1:12345",
  });
  profiles.closeConnections();
  expect(mocks.closeAll).toHaveBeenCalledOnce();
});

test("removes failed profile initialization so a later request can retry", async () => {
  const profiles = new AccountBrowserProfiles(account);
  mocks.listen.mockRejectedValueOnce(new Error("listen failed"));
  await expect(profiles.prepare(owner(1), input)).rejects.toThrow("listen failed");
  expect(profiles.partitions(1)).toEqual([]);
  expect(mocks.close).toHaveBeenCalledOnce();
  await expect(profiles.prepare(owner(1), input)).resolves.toHaveProperty("profilePartition");
});

test("rejects invalid Host identities before allocating a listener", async () => {
  const profiles = new AccountBrowserProfiles(account);
  await expect(profiles.prepare(owner(1), { ...input, hostId: "invalid" })).rejects.toThrow(
    "identity",
  );
  expect(mocks.listen).not.toHaveBeenCalled();
});

test("uses direct networking for the local daemon even when its connection is a relay", async () => {
  const profiles = new AccountBrowserProfiles(account, () => input.serverId);
  const profile = await profiles.prepare(owner(1), input);
  expect(profile.profilePartition).toBe("persist:ait-browser");
  expect(mocks.sessions.get(profile.profilePartition)?.setProxy).toHaveBeenCalledWith({
    mode: "direct",
  });
  expect(mocks.listen).not.toHaveBeenCalled();
});

test.each([
  { transportType: "rustTcp", url: "ws://192.168.1.2:7316/v1/ws" },
  { transportType: "ssh", host: "user@host", sshPort: 22, daemonPort: 7316 },
])("uses the authenticated daemon transport for $transportType workspaces", async (target) => {
  const profiles = new AccountBrowserProfiles(account, () => "local-daemon");
  const bearerToken = "test-token".repeat(4);
  const profile = await profiles.prepare(owner(1), {
    serverId: input.serverId,
    target,
    bearerToken,
  });
  expect(mocks.daemonTransport).toHaveBeenCalledWith({ target, bearerToken });
  expect(mocks.listen).toHaveBeenCalledWith("", "", input.serverId);
  expect(mocks.sessions.get(profile.profilePartition)?.setProxy).toHaveBeenCalledWith({
    proxyRules: "socks5://127.0.0.1:12345",
  });
});

test("rejects missing remote credentials without configuring a direct route", async () => {
  const profiles = new AccountBrowserProfiles(account);
  await expect(
    profiles.prepare(owner(1), {
      serverId: input.serverId,
      target: { transportType: "rustTcp", url: "ws://192.168.1.2:7316/v1/ws" },
    }),
  ).rejects.toThrow("Bearer token");
  expect(mocks.sessions.size).toBe(0);
  expect(mocks.listen).not.toHaveBeenCalled();
});

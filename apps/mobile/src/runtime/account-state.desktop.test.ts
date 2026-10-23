import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  ACCOUNT_HOST_OFFLINE_RETENTION_MS,
  accountCommand,
  loadAccountInfo,
  useAccountState,
} from "./account-state";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  loadRegistry: vi.fn(async () => {}),
  add: vi.fn(async () => {}),
  discover: vi.fn(async () => {}),
  setCenter: vi.fn(),
  setAvailable: vi.fn(),
}));
vi.mock("react-native", () => ({ Platform: { OS: "web" } }));
vi.mock("@/desktop/host", () => ({ getDesktopHost: () => ({ invoke: mocks.invoke }) }));
vi.mock("./native-account", () => ({ getNativeAccount: vi.fn(), subscribeNativeAccount: vi.fn() }));
vi.mock("./host-runtime", () => ({
  getHostRuntimeStore: () => ({
    loadRegistry: mocks.loadRegistry,
    addAccountRelayHost: mocks.add,
    addDiscoveredAccountRelayHosts: mocks.discover,
    setAccountRelayCenter: mocks.setCenter,
    setAvailableAccountRelayHosts: mocks.setAvailable,
  }),
}));

beforeEach(() => {
  vi.clearAllMocks();
  useAccountState.setState({ selected: null });
});

describe("desktop account host persistence", () => {
  it("loads account information through desktop IPC without modifying host state", async () => {
    const info = {
      loginName: "me@example.test",
      expiresAt: null,
      onlineClients: 1,
      onlineNodes: 0,
    };
    mocks.invoke.mockResolvedValue(info);
    await expect(loadAccountInfo()).resolves.toEqual(info);
    expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith("account_info");
    expect(mocks.discover).not.toHaveBeenCalled();
  });
  it("announces a Hub-only host without requiring legacy Relay metadata", async () => {
    mocks.invoke.mockResolvedValue({
      status: "online",
      center: "https://center.test/api",
      selected: null,
      stale: false,
      hosts: [
        {
          host_id: "remote",
          server_id: "server",
          instance_id: null,
          name: "Remote",
          online: true,
          hub_online: true,
          relay_modes: [],
        },
      ],
    });
    await accountCommand("account_status");
    expect(mocks.setAvailable).toHaveBeenLastCalledWith(["server"]);
  });

  it("announces usable hosts after discovery and clears availability when the account is stale", async () => {
    const snapshot = {
      status: "online",
      center: "https://center.test/api",
      selected: null,
      stale: false,
      hosts: [
        {
          host_id: "remote",
          server_id: "server",
          instance_id: "instance",
          name: "Remote",
          online: true,
          relay_modes: ["ait-rust-single-v1"],
        },
      ],
    };
    mocks.invoke.mockResolvedValue(snapshot);
    await accountCommand("account_status");
    expect(mocks.setAvailable).toHaveBeenLastCalledWith(["server"]);
    mocks.invoke.mockResolvedValue({ ...snapshot, stale: true });
    await accountCommand("account_status");
    expect(mocks.setAvailable).toHaveBeenLastCalledWith([], []);
    mocks.invoke.mockResolvedValue({ ...snapshot, status: "error" });
    await accountCommand("account_status");
    expect(mocks.setAvailable).toHaveBeenLastCalledWith([]);
    mocks.invoke.mockResolvedValue(snapshot);
    await accountCommand("account_status");
    expect(mocks.setAvailable).toHaveBeenLastCalledWith(["server"]);
  });

  it("imports all discovered hosts after browser login without selecting a target", async () => {
    const hosts = [
      {
        host_id: "first",
        server_id: "first-server",
        name: "First",
        online: true,
        instance_id: "first-instance",
        relay_modes: ["ait-rust-single-v1"],
      },
      {
        host_id: "second",
        server_id: "second-server",
        name: "Second",
        online: true,
        instance_id: "second-instance",
        relay_modes: ["ait-rust-single-v1"],
      },
    ];
    mocks.invoke.mockResolvedValueOnce({
      status: "online",
      center: "https://custom.test/api",
      selected: null,
      hosts,
    });
    await accountCommand("account_login_hosted");
    expect(mocks.discover).toHaveBeenCalledExactlyOnceWith(
      hosts,
      "https://custom.test/api",
      expect.any(Function),
      expect.any(Object),
    );
    expect(mocks.add).not.toHaveBeenCalled();
  });

  it("surfaces failed discovery persistence and lets login be retried", async () => {
    mocks.invoke.mockResolvedValue({
      status: "online",
      center: "https://custom.test/api",
      selected: null,
      hosts: [],
    });
    mocks.discover.mockRejectedValueOnce(new Error("Disk full"));
    await expect(accountCommand("account_login_hosted")).rejects.toThrow("Disk full");
    await accountCommand("account_login_hosted");
    expect(mocks.discover).toHaveBeenCalledTimes(2);
  });
  it("waits for the selected host to reach durable storage before returning to navigation", async () => {
    const host = { host_id: "remote", server_id: "server", name: "Computer" };
    mocks.invoke.mockResolvedValueOnce({
      status: "online",
      center: "https://custom.test/api",
      selected: host,
      hosts: [],
    });
    let finish!: () => void;
    mocks.add.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    let complete = false;
    const selection = accountCommand("account_select", { hostId: "remote" }).then(() => {
      complete = true;
    });
    await vi.waitFor(() => expect(mocks.add).toHaveBeenCalled());
    expect(mocks.add).toHaveBeenCalledWith(host, "https://custom.test/api");
    expect(complete).toBe(false);
    finish();
    await selection;
    expect(complete).toBe(true);
  });

  it("retains saved host records when logging out and restores their service on status recovery", async () => {
    mocks.invoke.mockResolvedValueOnce({ status: "logged_out", selected: null });
    await accountCommand("account_logout");
    expect(mocks.setCenter).toHaveBeenLastCalledWith(null);
    mocks.invoke.mockResolvedValueOnce({
      status: "online",
      center: "https://custom.test/api",
      selected: null,
      hosts: [],
    });
    await accountCommand("account_status");
    expect(mocks.setCenter).toHaveBeenLastCalledWith("https://custom.test/api");
    expect(mocks.add).not.toHaveBeenCalled();
  });

  it("surfaces failed host persistence so selection can be retried", async () => {
    const host = { host_id: "remote", server_id: "server", name: "Computer" };
    mocks.invoke.mockResolvedValue({
      status: "online",
      center: "https://custom.test/api",
      selected: host,
      hosts: [],
    });
    mocks.add.mockRejectedValueOnce(new Error("Disk full"));
    await expect(accountCommand("account_select", { hostId: "remote" })).rejects.toThrow(
      "Disk full",
    );
    await accountCommand("account_select", { hostId: "remote" });
    expect(mocks.add).toHaveBeenCalledTimes(2);
  });

  it("cancels browser login without waiting behind the desktop command queue", async () => {
    let finish!: (snapshot: unknown) => void;
    mocks.invoke.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const login = accountCommand("account_login_hosted");
    await vi.waitFor(() => expect(mocks.invoke).toHaveBeenCalled());
    mocks.invoke.mockResolvedValueOnce({ status: "logged_out", selected: null });
    await accountCommand("account_cancel_login");
    expect(mocks.invoke).toHaveBeenCalledWith("account_cancel_login", undefined);
    finish({ status: "logged_out", selected: null });
    await login;
  });
});

it("persists recent bound host records while only making online routable hosts available", async () => {
  const online = {
    host_id: "online",
    server_id: "online-server",
    instance_id: "instance",
    name: "Online",
    online: true,
    status: "offline",
    relay_modes: ["ait-rust-single-v1"],
  };
  const offline = {
    ...online,
    host_id: "offline",
    server_id: "offline-server",
    online: false,
    instance_id: null,
    relay_modes: [],
    offline_since: new Date(Date.now() - 60_000).toISOString(),
  };
  const expired = {
    ...offline,
    host_id: "expired",
    server_id: "expired-server",
    offline_since: new Date(Date.now() - ACCOUNT_HOST_OFFLINE_RETENTION_MS).toISOString(),
  };
  const manual = { ...offline, host_id: "manual", server_id: null };
  const unsupported = {
    ...online,
    host_id: "unsupported",
    server_id: "unsupported-server",
    relay_modes: [],
  };
  const unbound = { ...online, host_id: "unbound", server_id: "unbound-server", instance_id: null };
  mocks.invoke.mockResolvedValueOnce({
    status: "online",
    stale: false,
    center: "https://custom.test/api",
    selected: null,
    hosts: [online, offline, expired, manual, unsupported, unbound],
  });
  await accountCommand("account_status");
  expect(mocks.discover).toHaveBeenLastCalledWith(
    [online, offline, unsupported, unbound],
    "https://custom.test/api",
    expect.any(Function),
    { retainedHostIds: new Set(["online", "offline", "unsupported", "unbound"]) },
  );
  expect(mocks.setAvailable).toHaveBeenLastCalledWith(["online-server"]);
});

it("passes offline events through while the account snapshot is still stale", async () => {
  mocks.invoke.mockResolvedValueOnce({
    status: "connecting",
    stale: true,
    center: "https://custom.test/api",
    selected: null,
    confirmedHostIds: [],
    hosts: [{ host_id: "offline", server_id: "offline-server", online: false }],
  });
  await accountCommand("account_status");
  expect(mocks.discover).not.toHaveBeenCalled();
  expect(mocks.setAvailable).toHaveBeenCalledExactlyOnceWith([], ["offline-server"]);
});

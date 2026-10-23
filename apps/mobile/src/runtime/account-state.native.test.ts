import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HostProfile } from "@/types/host-connection";
import {
  accountCommand,
  loadAccountInfo,
  supportsAccountRelay,
  useAccountState,
} from "./account-state";

const mocks = vi.hoisted(() => ({
  manager: {
    loginWithBrowser: vi.fn(),
    cancelLogin: vi.fn(),
    logout: vi.fn(),
    select: vi.fn(),
    refresh: vi.fn(),
    getAccountInfo: vi.fn(),
    snapshot: vi.fn(),
  },
  loadRegistry: vi.fn(async () => {}),
  setHost: vi.fn(),
  discover: vi.fn(async () => {}),
  setCenter: vi.fn(),
  setAvailable: vi.fn(),
  removeConnection: vi.fn(),
  getHosts: vi.fn<() => HostProfile[]>(() => []),
  platform: "android",
}));
vi.mock("react-native", () => ({
  Platform: {
    get OS() {
      return mocks.platform;
    },
  },
}));
vi.mock("@/desktop/host", () => ({ getDesktopHost: () => undefined }));
vi.mock("./native-account", () => ({
  getNativeAccount: async () => mocks.manager,
  subscribeNativeAccount: vi.fn(),
}));
vi.mock("./host-runtime", () => ({
  getHostRuntimeStore: () => ({
    loadRegistry: mocks.loadRegistry,
    addAccountRelayHost: mocks.setHost,
    addDiscoveredAccountRelayHosts: mocks.discover,
    setAccountRelayCenter: mocks.setCenter,
    setAvailableAccountRelayHosts: mocks.setAvailable,
    removeConnection: mocks.removeConnection,
    getHosts: mocks.getHosts,
  }),
}));

beforeEach(() => {
  vi.clearAllMocks();
  mocks.getHosts.mockReturnValue([]);
  useAccountState.setState({ selected: null });
});

describe("native mobile account commands", () => {
  it("reads account information without triggering host synchronization", async () => {
    const info = {
      loginName: "me@example.test",
      expiresAt: null,
      onlineClients: 1,
      onlineNodes: 0,
    };
    mocks.manager.getAccountInfo.mockResolvedValue(info);
    await expect(loadAccountInfo()).resolves.toEqual(info);
    expect(mocks.manager.getAccountInfo).toHaveBeenCalledExactlyOnceWith();
    expect(mocks.discover).not.toHaveBeenCalled();
  });
  it.each(["android", "ios"])(
    "imports all discovered %s hosts after browser login",
    async (platform) => {
      mocks.platform = platform;
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
        {
          host_id: "offline",
          server_id: "offline-server",
          name: "Offline",
          online: false,
          instance_id: null,
          relay_modes: [],
        },
      ];
      mocks.manager.snapshot.mockReturnValue({
        status: "online",
        center: "https://center.test/api",
        selected: null,
        hosts: [...hosts, { host_id: "unbound", server_id: null, name: "Unbound", online: false }],
      });
      await accountCommand("account_login_hosted");
      expect(mocks.discover).toHaveBeenCalledExactlyOnceWith(
        hosts,
        "https://center.test/api",
        expect.any(Function),
        expect.any(Object),
      );
      expect(mocks.setAvailable).toHaveBeenCalledExactlyOnceWith(["first-server", "second-server"]);
      expect(mocks.manager.select).not.toHaveBeenCalled();
    },
  );

  it("waits for durable host storage before completing an explicit selection", async () => {
    const host = { host_id: "remote", server_id: "server", name: "Computer" };
    mocks.manager.snapshot.mockReturnValue({
      status: "online",
      center: "https://center.test/api",
      selected: host,
      hosts: [],
    });
    let finish!: () => void;
    mocks.setHost.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    let complete = false;
    const selection = accountCommand("account_select", { hostId: host.host_id }).then(() => {
      complete = true;
    });
    await vi.waitFor(() => expect(mocks.setHost).toHaveBeenCalled());
    expect(complete).toBe(false);
    finish();
    await selection;
    expect(complete).toBe(true);
  });

  it("does not recreate a removed host during account discovery or refresh", async () => {
    const host = { host_id: "remote", server_id: "server", name: "Computer" };
    mocks.manager.snapshot.mockReturnValue({
      status: "online",
      center: "https://center.test/api",
      selected: host,
      hosts: [],
    });
    await accountCommand("account_refresh");
    await accountCommand("account_status");
    expect(mocks.setHost).not.toHaveBeenCalled();
    expect(mocks.setCenter).toHaveBeenCalledWith("https://center.test/api");
  });

  it("removes only the selected service connection when the remote host is disconnected", async () => {
    const host = {
      host_id: "remote",
      server_id: "server",
      name: "Computer",
      online: true,
      status: "offline",
      node_id: "node",
      instance_id: "instance",
      platform: "linux",
      relay_modes: ["ait-rust-single-v1"],
    };
    useAccountState.setState({ selected: host });
    mocks.getHosts.mockReturnValue([
      {
        serverId: host.server_id,
        connections: [
          { id: "direct", type: "directTcp", endpoint: "remote.test:6767" },
          {
            id: "relay",
            type: "accountRelay",
            hostId: host.host_id,
            center: "https://center.test/api",
          },
        ],
      } as HostProfile,
    ]);
    mocks.manager.snapshot.mockReturnValue({
      status: "online",
      center: "https://center.test/api",
      selected: null,
      hosts: [],
    });
    await accountCommand("account_select", { hostId: null });
    expect(mocks.removeConnection).toHaveBeenCalledExactlyOnceWith("server", "relay");
    expect(mocks.setHost).not.toHaveBeenCalled();
  });
  it.each(["android", "ios"])(
    "cancels %s browser login without waiting behind the native account queue",
    async (platform) => {
      mocks.platform = platform;
      let finish!: () => void;
      mocks.manager.loginWithBrowser.mockImplementationOnce(
        () =>
          new Promise<void>((resolve) => {
            finish = resolve;
          }),
      );
      mocks.manager.snapshot.mockReturnValue({ status: "logged_out", selected: null });
      const login = accountCommand("account_login_hosted", { center: "https://center.test" });
      await vi.waitFor(() => expect(mocks.manager.loginWithBrowser).toHaveBeenCalled());
      await accountCommand("account_cancel_login");
      expect(mocks.manager.cancelLogin).toHaveBeenCalledOnce();
      finish();
      await login;
    },
  );
  it.each(["android", "ios"])(
    "enables the %s account entry and awaits the HostRuntime registry",
    async (platform) => {
      mocks.platform = platform;
      expect(supportsAccountRelay()).toBe(true);
      const host = { host_id: "remote", server_id: "server", name: "Computer" };
      mocks.manager.snapshot.mockReturnValue({
        status: "online",
        center: "https://center.test/api",
        selected: host,
        hosts: [],
      });
      let finishRegistry!: () => void;
      mocks.loadRegistry.mockReturnValueOnce(
        new Promise<void>((resolve) => {
          finishRegistry = resolve;
        }),
      );
      const command = accountCommand("account_select", { hostId: "remote" });
      await vi.waitFor(() => expect(mocks.loadRegistry).toHaveBeenCalled());
      expect(mocks.setHost).not.toHaveBeenCalled();
      finishRegistry();
      expect(await command).toMatchObject({ selected: host });
      expect(mocks.manager.select).toHaveBeenCalledWith("remote");
      expect(mocks.setHost).toHaveBeenCalledWith(host, "https://center.test/api");
      expect(useAccountState.getState().selected).toEqual(host);
    },
  );

  it.each(["android", "ios"])(
    "passes %s browser login and logout to the native authority",
    async (platform) => {
      mocks.platform = platform;
      await expect(accountCommand("account_login_hosted", { center: 123 })).rejects.toThrow(
        "Invalid service URL",
      );
      mocks.manager.snapshot.mockReturnValue({
        status: "connecting",
        hosts: [],
        stale: true,
        selected: null,
      });
      await accountCommand("account_login_hosted");
      expect(mocks.manager.loginWithBrowser).toHaveBeenCalledExactlyOnceWith("");
      mocks.manager.snapshot.mockReturnValue({ status: "logged_out", selected: null });
      await accountCommand("account_logout");
      expect(mocks.manager.logout).toHaveBeenCalledOnce();
      expect(mocks.setCenter).toHaveBeenLastCalledWith(null);
      expect(mocks.removeConnection).not.toHaveBeenCalled();
    },
  );

  it.each(["android", "ios"])("rejects the removed password command on %s", async (platform) => {
    mocks.platform = platform;
    await expect(
      accountCommand("account_login", {
        center: "https://center.test/api",
        email: "me@example.test",
        password: "test-password",
      }),
    ).rejects.toThrow("Unknown account command");
    expect(mocks.manager.loginWithBrowser).not.toHaveBeenCalled();
    expect(mocks.manager.logout).not.toHaveBeenCalled();
    expect(mocks.setHost).not.toHaveBeenCalled();
  });
});

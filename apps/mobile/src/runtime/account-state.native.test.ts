import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HostProfile } from "@/types/host-connection";
import {
  accountCommand,
  accountLoginMethods,
  supportsAccountRelay,
  useAccountState,
} from "./account-state";

const mocks = vi.hoisted(() => ({
  manager: {
    login: vi.fn(),
    loginMethods: vi.fn(),
    loginWithBrowser: vi.fn(),
    cancelLogin: vi.fn(),
    logout: vi.fn(),
    select: vi.fn(),
    refresh: vi.fn(),
    snapshot: vi.fn(),
  },
  boot: vi.fn(async () => {}),
  setHost: vi.fn(),
  setCenter: vi.fn(),
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
    boot: mocks.boot,
    addAccountRelayHost: mocks.setHost,
    setAccountRelayCenter: mocks.setCenter,
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
  it("waits for durable host storage before completing an explicit selection", async () => {
    const host = { host_id: "remote", server_id: "server", name: "Computer" };
    mocks.manager.snapshot.mockReturnValue({
      status: "online",
      center: "https://center.test/api",
      selected: host,
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
    });
    await accountCommand("account_select", { hostId: null });
    expect(mocks.removeConnection).toHaveBeenCalledExactlyOnceWith("server", "relay");
    expect(mocks.setHost).not.toHaveBeenCalled();
  });
  it.each(["android", "ios"])(
    "discovers hosted login for %s through the native authority",
    async (platform) => {
      mocks.platform = platform;
      mocks.manager.loginMethods.mockResolvedValueOnce({ hosted: true });
      await expect(accountLoginMethods("https://center.test/api")).resolves.toEqual({
        hosted: true,
      });
      expect(mocks.manager.loginMethods).toHaveBeenCalledExactlyOnceWith("https://center.test/api");
    },
  );
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
    "enables the %s account entry and awaits HostRuntime",
    async (platform) => {
      mocks.platform = platform;
      expect(supportsAccountRelay()).toBe(true);
      const host = { host_id: "remote", server_id: "server", name: "Computer" };
      mocks.manager.snapshot.mockReturnValue({
        status: "online",
        center: "https://center.test/api",
        selected: host,
      });
      let finishBoot!: () => void;
      mocks.boot.mockReturnValueOnce(
        new Promise<void>((resolve) => {
          finishBoot = resolve;
        }),
      );
      const command = accountCommand("account_select", { hostId: "remote" });
      await vi.waitFor(() => expect(mocks.boot).toHaveBeenCalled());
      expect(mocks.setHost).not.toHaveBeenCalled();
      finishBoot();
      expect(await command).toMatchObject({ selected: host });
      expect(mocks.manager.select).toHaveBeenCalledWith("remote");
      expect(mocks.setHost).toHaveBeenCalledWith(host, "https://center.test/api");
      expect(useAccountState.getState().selected).toEqual(host);
    },
  );

  it.each(["android", "ios"])(
    "passes %s login credentials to the native authority",
    async (platform) => {
      mocks.platform = platform;
      await expect(accountCommand("account_login", { email: "me" })).rejects.toThrow(
        "Invalid login",
      );
      mocks.manager.snapshot.mockReturnValue({ status: "connecting", selected: null });
      await accountCommand("account_login", { email: "me@example.test", password: " secret " });
      expect(mocks.manager.login).toHaveBeenCalledWith("", "me@example.test", " secret ");
      mocks.manager.snapshot.mockReturnValue({ status: "logged_out", selected: null });
      await accountCommand("account_logout");
      expect(mocks.manager.logout).toHaveBeenCalledOnce();
      expect(mocks.setCenter).toHaveBeenLastCalledWith(null);
      expect(mocks.removeConnection).not.toHaveBeenCalled();
    },
  );
});

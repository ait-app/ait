import { beforeEach, describe, expect, it, vi } from "vitest";
import { accountCommand, supportsAccountRelay, useAccountState } from "./account-state";

const mocks = vi.hoisted(() => ({
  manager: {
    login: vi.fn(),
    logout: vi.fn(),
    select: vi.fn(),
    refresh: vi.fn(),
    snapshot: vi.fn(),
  },
  boot: vi.fn(async () => {}),
  setHost: vi.fn(),
}));
vi.mock("react-native", () => ({ Platform: { OS: "android" } }));
vi.mock("@/desktop/host", () => ({ getDesktopHost: () => undefined }));
vi.mock("./native-account", () => ({
  getNativeAccount: async () => mocks.manager,
  subscribeNativeAccount: vi.fn(),
}));
vi.mock("./host-runtime", () => ({
  getHostRuntimeStore: () => ({ boot: mocks.boot, setAccountRelayHost: mocks.setHost }),
}));

beforeEach(() => vi.clearAllMocks());

describe("Android account commands", () => {
  it("enables the account entry without Electron and awaits HostRuntime before navigating", async () => {
    expect(supportsAccountRelay()).toBe(true);
    const host = { host_id: "remote", server_id: "server", name: "Computer" };
    mocks.manager.snapshot.mockReturnValue({ status: "online", selected: host });
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
    expect(mocks.setHost).toHaveBeenCalledWith(host);
    expect(useAccountState.getState().selected).toEqual(host);
  });

  it("passes login credentials to the native authority and recovers the command queue after errors", async () => {
    await expect(accountCommand("account_login", { email: "me" })).rejects.toThrow("Invalid login");
    mocks.manager.snapshot.mockReturnValue({ status: "connecting", selected: null });
    await accountCommand("account_login", { email: "me@example.test", password: " secret " });
    expect(mocks.manager.login).toHaveBeenCalledWith("", "me@example.test", " secret ");
    mocks.manager.snapshot.mockReturnValue({ status: "logged_out", selected: null });
    await accountCommand("account_logout");
    expect(mocks.manager.logout).toHaveBeenCalledOnce();
    expect(mocks.setHost).toHaveBeenLastCalledWith(null);
  });
});

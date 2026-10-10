import { beforeEach, describe, expect, it, vi } from "vitest";
import { accountCommand, useAccountState } from "./account-state";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  boot: vi.fn(async () => {}),
  add: vi.fn(async () => {}),
  setCenter: vi.fn(),
}));
vi.mock("react-native", () => ({ Platform: { OS: "web" } }));
vi.mock("@/desktop/host", () => ({ getDesktopHost: () => ({ invoke: mocks.invoke }) }));
vi.mock("./native-account", () => ({ getNativeAccount: vi.fn(), subscribeNativeAccount: vi.fn() }));
vi.mock("./host-runtime", () => ({
  getHostRuntimeStore: () => ({
    boot: mocks.boot,
    addAccountRelayHost: mocks.add,
    setAccountRelayCenter: mocks.setCenter,
  }),
}));

beforeEach(() => {
  vi.clearAllMocks();
  useAccountState.setState({ selected: null });
});

describe("desktop account host persistence", () => {
  it("waits for the selected host to reach durable storage before returning to navigation", async () => {
    const host = { host_id: "remote", server_id: "server", name: "Computer" };
    mocks.invoke.mockResolvedValueOnce({
      status: "online",
      center: "https://custom.test/api",
      selected: host,
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

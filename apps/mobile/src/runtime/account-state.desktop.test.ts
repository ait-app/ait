import { beforeEach, describe, expect, it, vi } from "vitest";
import { accountCommand, useAccountState } from "./account-state";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  boot: vi.fn(async () => {}),
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
    boot: mocks.boot,
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
  it("announces usable hosts after discovery and clears availability when the account is stale", async () => {
    const snapshot = {
      status: "online",
      center: "https://center.test/api",
      selected: null,
      stale: false,
      hosts: [{ host_id: "remote", server_id: "server", name: "Remote" }],
    };
    mocks.invoke.mockResolvedValue(snapshot);
    await accountCommand("account_status");
    expect(mocks.setAvailable).toHaveBeenLastCalledWith(["server"]);
    mocks.invoke.mockResolvedValue({ ...snapshot, stale: true });
    await accountCommand("account_status");
    expect(mocks.setAvailable).toHaveBeenLastCalledWith([]);
    mocks.invoke.mockResolvedValue({ ...snapshot, status: "error" });
    await accountCommand("account_status");
    expect(mocks.setAvailable).toHaveBeenLastCalledWith([]);
    mocks.invoke.mockResolvedValue(snapshot);
    await accountCommand("account_status");
    expect(mocks.setAvailable).toHaveBeenLastCalledWith(["server"]);
  });

  it("imports all discovered hosts after browser login without selecting a target", async () => {
    const hosts = [
      { host_id: "first", server_id: "first-server", name: "First" },
      { host_id: "second", server_id: "second-server", name: "Second" },
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

import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  disconnectOnlineServiceHost,
  synchronizeOnlineServiceHost,
  useOnlineServiceHostSync,
  reconcileOnlineServiceHostsAfterLogout,
} from "./online-service-host-sync";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  status: vi.fn(),
  connect: vi.fn(),
  disconnect: vi.fn(),
  accountStatus: "online",
  desktop: true,
  platform: "ios",
  publishHost: vi.fn(),
  unpublishHost: vi.fn(),
}));
vi.mock("react-native", () => ({
  Platform: {
    get OS() {
      return mocks.platform;
    },
  },
}));
vi.mock("@/desktop/host", () => ({
  getDesktopHost: () => (mocks.desktop ? { invoke: mocks.invoke } : undefined),
}));
vi.mock("./account-state", () => ({
  serializeNativeAccountCommand: (work: () => Promise<unknown>) => work(),
  useAccountState: Object.assign(vi.fn(), { getState: () => ({ status: mocks.accountStatus }) }),
}));
vi.mock("./native-account", () => ({
  getNativeAccount: async () => ({
    publishHost: mocks.publishHost,
    unpublishHost: mocks.unpublishHost,
  }),
}));
vi.mock("./host-runtime", () => ({
  getHostRuntimeStore: () => ({
    getClient: () => ({
      isConnected: true,
      getOnlineServiceStatus: mocks.status,
      connectOnlineService: mocks.connect,
      disconnectOnlineService: mocks.disconnect,
    }),
  }),
}));
const status = (serverId = "first", online = false, instanceId = "instance-1") => ({
  serverId,
  instanceId,
  platform: "linux",
  status: { online, connecting: false, epoch: null, error: null },
});
const grant = {
  center_url: "https://example.test/api",
  control_ticket: "a".repeat(64),
  node_session_id: "00000000-0000-4000-8000-000000000001",
};

beforeEach(() => {
  vi.resetAllMocks();
  mocks.accountStatus = "online";
  mocks.desktop = true;
  useOnlineServiceHostSync.setState({ hosts: {} });
  mocks.status.mockResolvedValue(status());
  mocks.invoke.mockResolvedValue(grant);
  mocks.connect.mockResolvedValue({
    ...status(),
    status: { ...status().status, connecting: true },
  });
  mocks.disconnect.mockResolvedValue(status());
  mocks.publishHost.mockResolvedValue(grant);
});

describe("host online service synchronization", () => {
  it("uses the iOS native account authority to synchronize and disconnect a host", async () => {
    mocks.desktop = false;
    await synchronizeOnlineServiceHost("first", "First", true);
    expect(mocks.publishHost).toHaveBeenCalledWith(
      { serverId: "first", instanceId: "instance-1", platform: "linux", name: "First" },
      true,
    );
    expect(mocks.connect).toHaveBeenCalledWith(grant);
    await disconnectOnlineServiceHost("first");
    expect(mocks.unpublishHost).toHaveBeenCalledWith("first");
    expect(mocks.disconnect).toHaveBeenCalledOnce();
  });

  it("reads status without publishing, then sends a grant only to the explicitly selected daemon", async () => {
    await synchronizeOnlineServiceHost("first", "First");
    expect(mocks.invoke).not.toHaveBeenCalled();
    await synchronizeOnlineServiceHost("first", "First", true);
    expect(mocks.invoke).toHaveBeenCalledWith("account_host_sync", {
      serverId: "first",
      instanceId: "instance-1",
      platform: "linux",
      name: "First",
      needsGrant: true,
    });
    expect(mocks.connect).toHaveBeenCalledExactlyOnceWith(grant);
    expect(useOnlineServiceHostSync.getState().hosts.first).toMatchObject({
      enabled: true,
      busy: false,
      status: { status: { connecting: true } },
    });
    mocks.status.mockResolvedValue(status("first", true));
    mocks.invoke.mockResolvedValue(null);
    await synchronizeOnlineServiceHost("first", "First");
    expect(mocks.invoke).toHaveBeenLastCalledWith(
      "account_host_sync",
      expect.objectContaining({ needsGrant: false }),
    );
    expect(mocks.connect).toHaveBeenCalledOnce();
    await disconnectOnlineServiceHost("first");
    expect(mocks.invoke).toHaveBeenLastCalledWith("account_host_disconnect", { serverId: "first" });
    expect(mocks.disconnect).toHaveBeenCalledOnce();
    expect(useOnlineServiceHostSync.getState().hosts.first.enabled).toBe(false);
  });

  it("retains enabled intent after a connection failure so background polling can retry", async () => {
    mocks.connect.mockRejectedValueOnce(new Error("Disconnected"));
    await synchronizeOnlineServiceHost("first", "First", true);
    expect(useOnlineServiceHostSync.getState().hosts.first).toMatchObject({
      enabled: true,
      busy: false,
      error: "Disconnected",
    });
    await synchronizeOnlineServiceHost("first", "First");
    expect(mocks.connect).toHaveBeenCalledTimes(2);
    expect(useOnlineServiceHostSync.getState().hosts.first.error).toBeNull();
  });

  it("rejects mismatched identities and does not publish while signed out", async () => {
    mocks.status.mockResolvedValue(status("other"));
    await synchronizeOnlineServiceHost("first", "First", true);
    expect(mocks.invoke).not.toHaveBeenCalled();
    expect(useOnlineServiceHostSync.getState().hosts.first.error).toContain("identity has changed");
    mocks.accountStatus = "logged_out";
    await synchronizeOnlineServiceHost("first", "First", true);
    expect(mocks.invoke).not.toHaveBeenCalled();
  });

  it("preserves remote synchronization after logout and still allows an explicit stop", async () => {
    await synchronizeOnlineServiceHost("first", "First", true);
    useOnlineServiceHostSync.setState(({ hosts }) => ({
      hosts: { ...hosts, binding: { ...hosts.first } },
    }));
    mocks.accountStatus = "logged_out";
    reconcileOnlineServiceHostsAfterLogout(["first"]);
    expect(useOnlineServiceHostSync.getState().hosts.first.enabled).toBe(true);
    expect(useOnlineServiceHostSync.getState().hosts.binding.enabled).toBe(false);
    mocks.connect.mockClear();
    await synchronizeOnlineServiceHost("first", "First");
    expect(mocks.connect).toHaveBeenCalledExactlyOnceWith(grant);
    await disconnectOnlineServiceHost("first");
    expect(mocks.invoke).toHaveBeenLastCalledWith("account_host_disconnect", { serverId: "first" });
    expect(mocks.disconnect).toHaveBeenCalledOnce();
    expect(useOnlineServiceHostSync.getState().hosts.first.enabled).toBe(false);
  });
});

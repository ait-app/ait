/** @vitest-environment jsdom */
import React from "react";
import { cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  disconnectOnlineServiceHost,
  synchronizeOnlineServiceHost,
  useOnlineServiceHostSync,
  reconcileOnlineServiceHostsAfterLogout,
  synchronizeDefaultDesktopHost,
  OnlineServiceHostSyncLifecycle,
} from "./online-service-host-sync";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  status: vi.fn(),
  connect: vi.fn(),
  disconnect: vi.fn(),
  accountStatus: "online",
  syncBuiltInDaemon: true,
  daemonStatus: vi.fn(),
  connected: true,
  supported: true,
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
vi.mock("@/desktop/daemon/desktop-daemon", () => ({ getDesktopDaemonStatus: mocks.daemonStatus }));
vi.mock("./account-state", () => ({
  serializeNativeAccountCommand: (work: () => Promise<unknown>) => work(),
  useAccountState: Object.assign(
    (selector: (state: unknown) => unknown) =>
      selector({
        status: mocks.accountStatus,
        syncBuiltInDaemon: mocks.syncBuiltInDaemon,
      }),
    {
      getState: () => ({ status: mocks.accountStatus, syncBuiltInDaemon: mocks.syncBuiltInDaemon }),
    },
  ),
}));
vi.mock("./native-account", () => ({
  getNativeAccount: async () => ({
    publishHost: mocks.publishHost,
    unpublishHost: mocks.unpublishHost,
  }),
}));
vi.mock("./host-runtime", () => ({
  getHostRuntimeStore: () => ({
    getHosts: () => [
      { serverId: "first", label: "Built-in daemon" },
      { serverId: "remote", label: "Remote" },
    ],
    getClient: () => ({
      isConnected: mocks.connected,
      getLastServerInfoMessage: () => ({ features: { onlineServiceSync: mocks.supported } }),
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
  mocks.syncBuiltInDaemon = true;
  mocks.connected = true;
  mocks.supported = true;
  mocks.daemonStatus.mockResolvedValue({ status: "running", serverId: "first" });
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
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("host online service synchronization", () => {
  it("starts synchronization after login without opening settings and keeps a manual stop offline", async () => {
    mocks.accountStatus = "logged_out";
    const view = render(React.createElement(OnlineServiceHostSyncLifecycle));
    expect(mocks.invoke).not.toHaveBeenCalled();
    mocks.accountStatus = "online";
    view.rerender(React.createElement(OnlineServiceHostSyncLifecycle));
    await waitFor(() => expect(mocks.connect).toHaveBeenCalledExactlyOnceWith(grant));
    await disconnectOnlineServiceHost("first");
    mocks.syncBuiltInDaemon = false;
    mocks.invoke.mockClear();
    vi.useFakeTimers();
    view.rerender(React.createElement(OnlineServiceHostSyncLifecycle));
    await vi.advanceTimersByTimeAsync(10000);
    expect(mocks.invoke).not.toHaveBeenCalled();
  });
  it("automatically publishes only the desktop's built-in daemon after sign-in", async () => {
    await synchronizeDefaultDesktopHost();
    expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith(
      "account_host_sync",
      expect.objectContaining({ serverId: "first", name: "Built-in daemon" }),
    );
    expect(mocks.connect).toHaveBeenCalledExactlyOnceWith(grant);
  });

  it("respects a persisted manual stop and can explicitly re-enable synchronization", async () => {
    mocks.syncBuiltInDaemon = false;
    await synchronizeDefaultDesktopHost();
    expect(mocks.daemonStatus).not.toHaveBeenCalled();
    expect(mocks.invoke).not.toHaveBeenCalled();
    await synchronizeOnlineServiceHost("first", "Built-in daemon", true);
    expect(mocks.connect).toHaveBeenCalledExactlyOnceWith(grant);
  });

  it("does not auto-publish on mobile or before account registration completes", async () => {
    mocks.desktop = false;
    await synchronizeDefaultDesktopHost();
    mocks.desktop = true;
    mocks.accountStatus = "logged_out";
    await synchronizeDefaultDesktopHost();
    mocks.accountStatus = "connecting";
    await synchronizeDefaultDesktopHost();
    expect(mocks.daemonStatus).not.toHaveBeenCalled();
    expect(mocks.invoke).not.toHaveBeenCalled();
  });

  it("waits for the local daemon and its connection, then retries after startup", async () => {
    mocks.daemonStatus.mockResolvedValueOnce({ status: "stopped", serverId: "first" });
    await synchronizeDefaultDesktopHost();
    mocks.connected = false;
    await synchronizeDefaultDesktopHost();
    mocks.connected = true;
    mocks.supported = false;
    await synchronizeDefaultDesktopHost();
    mocks.supported = true;
    expect(mocks.invoke).not.toHaveBeenCalled();
    await synchronizeDefaultDesktopHost();
    expect(mocks.connect).toHaveBeenCalledExactlyOnceWith(grant);
  });

  it("does not publish when manual stop arrives during the daemon status request", async () => {
    mocks.daemonStatus.mockImplementationOnce(async () => {
      mocks.syncBuiltInDaemon = false;
      return { status: "running", serverId: "first" };
    });
    await synchronizeDefaultDesktopHost();
    expect(mocks.invoke).not.toHaveBeenCalled();
  });

  it("reuses the stable host identity after the desktop daemon restarts", async () => {
    await synchronizeDefaultDesktopHost();
    mocks.status.mockResolvedValue(status("first", false, "instance-2"));
    await synchronizeDefaultDesktopHost();
    expect(mocks.invoke).toHaveBeenLastCalledWith(
      "account_host_sync",
      expect.objectContaining({ serverId: "first", instanceId: "instance-2" }),
    );
  });
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
      enableBuiltInDaemon: true,
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

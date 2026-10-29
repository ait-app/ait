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
  disconnect: vi.fn(),
  accountStatus: "online",
  syncBuiltInDaemon: true,
  daemonStatus: vi.fn(),
  connected: true,
  supported: true,
  desktop: true,
  platform: "ios",
  authorizeHost: vi.fn(),
  publication: vi.fn(),
  authorize: vi.fn(),
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
    authorizeHost: mocks.authorizeHost,
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
      getPublicationStatus: mocks.publication,
      authorizePublication: mocks.authorize,
      disconnectPublication: mocks.disconnect,
    }),
  }),
}));
const status = () => ({
  serverId: "first",
  instanceId: "instance-1",
  platform: "darwin",
  management: { mode: "managed", autonomous: true },
  status: { online: false, connecting: false, epoch: null, error: null },
});
const publication = (phase = "unauthorized") => ({
  ...status(),
  management: {
    ...status().management,
    phase,
    binding: ["unauthorized", "disconnected"].includes(phase) ? null : { server_id: "first" },
  },
  hub: {
    status: {
      online: phase === "running",
      connecting: phase === "connecting",
      error: null,
      rejected: null,
    },
  },
  relay: status().status,
});
const authorization = {
  kind: "device_code",
  device_code: "a".repeat(64),
  user_code: "ABCD-0123-ABCD",
  request_id: "00000000-0000-4000-8000-000000000001",
  expires_at: new Date(Date.now() + 600_000).toISOString(),
  interval: 5,
};

beforeEach(() => {
  vi.resetAllMocks();
  mocks.accountStatus = "online";
  mocks.desktop = true;
  mocks.syncBuiltInDaemon = true;
  mocks.connected = true;
  mocks.supported = true;
  mocks.daemonStatus.mockResolvedValue({ status: "running", serverId: "first" });
  reconcileOnlineServiceHostsAfterLogout([]);
  useOnlineServiceHostSync.setState({ hosts: {} });
  mocks.status.mockResolvedValue(status());
  mocks.publication.mockResolvedValue(publication());
  mocks.invoke.mockResolvedValue(authorization);
  mocks.authorizeHost.mockResolvedValue(authorization);
  mocks.authorize.mockResolvedValue(publication("connecting"));
  mocks.disconnect.mockResolvedValue(publication("disconnected"));
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("daemon-owned publication", () => {
  it("hands an approval to the built-in daemon after login without opening Settings", async () => {
    mocks.accountStatus = "logged_out";
    const view = render(React.createElement(OnlineServiceHostSyncLifecycle));
    expect(mocks.invoke).not.toHaveBeenCalled();
    mocks.accountStatus = "online";
    view.rerender(React.createElement(OnlineServiceHostSyncLifecycle));
    await waitFor(() =>
      expect(mocks.authorize).toHaveBeenCalledExactlyOnceWith(authorization, "initial"),
    );
    expect(mocks.invoke).toHaveBeenCalledWith(
      "account_host_sync",
      expect.objectContaining({ serverId: "first", needsAuthorization: true }),
    );
  });

  it("does not automatically reauthorize an externally disconnected daemon, including after restart", async () => {
    mocks.publication.mockResolvedValue(publication("disconnected"));
    await synchronizeDefaultDesktopHost();
    await synchronizeOnlineServiceHost("first", "SHLP");
    expect(mocks.invoke).not.toHaveBeenCalled();
    expect(mocks.authorize).not.toHaveBeenCalled();
    expect(useOnlineServiceHostSync.getState().hosts.first?.enabled).toBe(false);
  });

  it("submits automatic authorization conditionally even if a stale status reply arrives after disconnect", async () => {
    let phase = "unauthorized";
    mocks.publication.mockImplementation(async () => {
      const snapshot = publication(phase);
      phase = "disconnected"; // External DELETE wins after the status read.
      return snapshot;
    });
    mocks.authorize.mockImplementation(async (_authorization, mode) => {
      if (mode !== "initial" || phase === "unauthorized") phase = "connecting";
      return publication(phase);
    });
    await synchronizeDefaultDesktopHost();
    expect(mocks.authorize).toHaveBeenCalledExactlyOnceWith(authorization, "initial");
    expect(phase).toBe("disconnected");
    expect(useOnlineServiceHostSync.getState().hosts.first?.enabled).toBe(false);
  });

  it("requires login again after disconnect, then authorizes both links together", async () => {
    mocks.accountStatus = "logged_out";
    mocks.publication.mockResolvedValue(publication("disconnected"));
    await synchronizeOnlineServiceHost("first", "SHLP", true);
    expect(mocks.authorize).not.toHaveBeenCalled();
    expect(mocks.invoke).not.toHaveBeenCalled();
    mocks.accountStatus = "online";
    await synchronizeOnlineServiceHost("first", "SHLP", true);
    expect(mocks.authorize).toHaveBeenCalledExactlyOnceWith(authorization, "explicit");
    expect(mocks.invoke).toHaveBeenCalledWith(
      "account_host_sync",
      expect.objectContaining({ needsAuthorization: true, enableBuiltInDaemon: true }),
    );
    expect(useOnlineServiceHostSync.getState().hosts.first?.enabled).toBe(true);
  });

  it("keeps a persisted manual stop until explicitly enabled", async () => {
    mocks.syncBuiltInDaemon = false;
    await synchronizeDefaultDesktopHost();
    expect(mocks.daemonStatus).not.toHaveBeenCalled();
    await synchronizeOnlineServiceHost("first", "SHLP", true);
    expect(mocks.authorize).toHaveBeenCalledExactlyOnceWith(authorization, "explicit");
  });

  it("disconnects through the daemon and clears shared authorization and both links", async () => {
    await disconnectOnlineServiceHost("first");
    expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith("account_host_disconnect", {
      serverId: "first",
    });
    expect(mocks.disconnect).toHaveBeenCalledOnce();
    expect(mocks.authorize).not.toHaveBeenCalled();
    expect(useOnlineServiceHostSync.getState().hosts.first).toMatchObject({
      enabled: false,
      publication: {
        hub: { status: { online: false } },
        relay: { online: false },
        management: { phase: "disconnected", binding: null },
      },
    });
  });

  it("retries a lost handoff response with the same issuance identity", async () => {
    mocks.authorize.mockRejectedValueOnce(new Error("Connection lost"));
    await synchronizeOnlineServiceHost("first", "SHLP", true);
    expect(useOnlineServiceHostSync.getState().hosts.first?.enabled).toBe(false);
    await synchronizeOnlineServiceHost("first", "SHLP", true);
    expect(mocks.authorize).toHaveBeenNthCalledWith(1, authorization, "explicit");
    expect(mocks.authorize).toHaveBeenNthCalledWith(2, authorization, "explicit");
    expect(mocks.invoke).toHaveBeenLastCalledWith(
      "account_host_sync",
      expect.objectContaining({ needsAuthorization: false }),
    );
  });

  it("discards an unacknowledged device code when explicitly disconnected", async () => {
    mocks.authorize.mockRejectedValueOnce(new Error("Connection lost"));
    await synchronizeOnlineServiceHost("first", "SHLP", true);
    await disconnectOnlineServiceHost("first");
    mocks.publication.mockResolvedValue(publication("disconnected"));
    const fresh = { ...authorization, device_code: "b".repeat(64) };
    mocks.invoke.mockResolvedValue(fresh);
    await synchronizeOnlineServiceHost("first", "SHLP", true);
    expect(mocks.invoke).toHaveBeenLastCalledWith(
      "account_host_sync",
      expect.objectContaining({ needsAuthorization: true }),
    );
    expect(mocks.authorize).toHaveBeenLastCalledWith(fresh, "explicit");
  });

  it("requires explicit authorization after revocation instead of automatically re-enrolling", async () => {
    mocks.publication.mockResolvedValue(publication("reauthorization_required"));
    await synchronizeDefaultDesktopHost();
    expect(mocks.authorize).not.toHaveBeenCalled();
    await synchronizeOnlineServiceHost("first", "SHLP", true);
    expect(mocks.authorize).toHaveBeenCalledExactlyOnceWith(authorization, "explicit");
  });

  it.each(["storage_error", "protocol_error"])(
    "reauthorizes explicitly after %s",
    async (phase) => {
      mocks.publication.mockResolvedValue(publication(phase));
      await synchronizeDefaultDesktopHost();
      expect(mocks.authorize).not.toHaveBeenCalled();
      await synchronizeOnlineServiceHost("first", "SHLP", true);
      expect(mocks.authorize).toHaveBeenCalledExactlyOnceWith(authorization, "explicit");
    },
  );

  it("uses the native account only to approve a selected daemon", async () => {
    mocks.desktop = false;
    await synchronizeOnlineServiceHost("first", "SHLP", true);
    expect(mocks.authorizeHost).toHaveBeenCalledExactlyOnceWith({
      serverId: "first",
      instanceId: "instance-1",
      name: "SHLP",
      platform: "darwin",
    });
    expect(mocks.authorize).toHaveBeenCalledExactlyOnceWith(authorization, "explicit");
    expect(mocks.invoke).not.toHaveBeenCalled();
  });

  it("does not disturb daemon publication on account logout", async () => {
    mocks.publication.mockResolvedValue(publication("running"));
    await synchronizeOnlineServiceHost("first", "SHLP");
    reconcileOnlineServiceHostsAfterLogout([]);
    expect(useOnlineServiceHostSync.getState().hosts.first?.enabled).toBe(true);
    expect(mocks.disconnect).not.toHaveBeenCalled();
  });

  it("clears a cached publication when the host no longer supports autonomous publication", async () => {
    mocks.publication.mockResolvedValue(publication("running"));
    await synchronizeOnlineServiceHost("first", "SHLP");
    mocks.status.mockResolvedValue({ ...status(), management: { mode: "managed" } });
    await synchronizeOnlineServiceHost("first", "SHLP");
    expect(useOnlineServiceHostSync.getState().hosts.first).toMatchObject({
      enabled: false,
      publication: undefined,
    });
  });

  it("does not authorize a changed identity or an old daemon", async () => {
    mocks.status.mockResolvedValueOnce({ ...status(), serverId: "other" });
    await synchronizeOnlineServiceHost("first", "SHLP", true);
    expect(useOnlineServiceHostSync.getState().hosts.first?.error).toContain("identity");
    mocks.status.mockResolvedValueOnce({ ...status(), management: { mode: "managed" } });
    await synchronizeOnlineServiceHost("first", "SHLP", true);
    expect(useOnlineServiceHostSync.getState().hosts.first?.error).toContain("Update");
    expect(mocks.invoke).not.toHaveBeenCalled();
  });
});

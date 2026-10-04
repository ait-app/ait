import { useEffect } from "react";
import { Platform } from "react-native";
import { create } from "zustand";
import type { RelayControlGrant, RelayStatus } from "@ait/protocol/relay";
import { getDesktopHost } from "@/desktop/host";
import { getNativeAccount } from "./native-account";
import { getHostRuntimeStore } from "./host-runtime";
import { serializeNativeAccountCommand, useAccountState } from "./account-state";

interface HostSyncState {
  enabled: boolean;
  busy: boolean;
  status: RelayStatus | null;
  error: string | null;
}
const EMPTY_STATE: HostSyncState = { enabled: false, busy: false, status: null, error: null };
export const useOnlineServiceHostSync = create<{ hosts: Record<string, HostSyncState> }>(() => ({
  hosts: {},
}));
let generation = 0;

function update(serverId: string, patch: Partial<HostSyncState>) {
  useOnlineServiceHostSync.setState(({ hosts }) => ({
    hosts: { ...hosts, [serverId]: { ...(hosts[serverId] ?? EMPTY_STATE), ...patch } },
  }));
}

async function hostCommand(
  command: "account_host_sync" | "account_host_disconnect",
  args: Record<string, unknown>,
): Promise<RelayControlGrant | null> {
  const desktop = getDesktopHost();
  if (desktop?.invoke) return desktop.invoke(command, args) as Promise<RelayControlGrant | null>;
  if (Platform.OS !== "android" && Platform.OS !== "ios")
    throw new Error("Online service synchronization requires the desktop or native mobile app.");
  return serializeNativeAccountCommand(async () => {
    const manager = await getNativeAccount();
    if (command === "account_host_disconnect") {
      await manager.unpublishHost(args.serverId as string);
      return null;
    }
    return manager.publishHost(
      {
        serverId: args.serverId as string,
        instanceId: args.instanceId as string,
        name: args.name as string,
        platform: args.platform as string,
      },
      args.needsGrant !== false,
    );
  });
}

/** Read status from the selected daemon, and maintain only explicitly enabled publications. */
export async function synchronizeOnlineServiceHost(
  serverId: string,
  name: string,
  enable = false,
): Promise<void> {
  const previous = useOnlineServiceHostSync.getState().hosts[serverId] ?? EMPTY_STATE;
  if (previous.busy) return;
  const client = getHostRuntimeStore().getClient(serverId);
  if (!client?.isConnected) return;
  if (enable && useAccountState.getState().status === "logged_out") return;
  const current = generation;
  update(serverId, { busy: true, error: null, enabled: enable || previous.enabled });
  try {
    let status = await client.getOnlineServiceStatus();
    if (status.serverId !== serverId)
      throw new Error("The host identity has changed. Reconnect first.");
    if (current !== generation) return;
    const enabled = enable || previous.enabled;
    if (enabled) {
      const grant = await hostCommand("account_host_sync", {
        serverId,
        instanceId: status.instanceId,
        platform: status.platform,
        name,
        needsGrant: !status.status.online && !status.status.connecting,
      });
      if (current !== generation) return;
      if (grant) status = await client.connectOnlineService(grant);
    }
    if (current === generation) update(serverId, { status, enabled });
  } catch (error) {
    if (current === generation)
      update(serverId, {
        error: error instanceof Error ? error.message : "Host synchronization failed.",
      });
  } finally {
    if (current === generation) update(serverId, { busy: false });
  }
}

export async function disconnectOnlineServiceHost(serverId: string): Promise<void> {
  const client = getHostRuntimeStore().getClient(serverId);
  if (useOnlineServiceHostSync.getState().hosts[serverId]?.busy || !client?.isConnected) return;
  const current = generation;
  update(serverId, { busy: true, error: null });
  try {
    // Revoke the lease even if the transport disappears while stopping the connector.
    await hostCommand("account_host_disconnect", { serverId });
    if (current !== generation) return;
    update(serverId, { enabled: false });
    const status = await client.disconnectOnlineService();
    if (current === generation) update(serverId, { status });
  } catch (error) {
    if (current === generation)
      update(serverId, {
        error: error instanceof Error ? error.message : "Host synchronization failed.",
      });
  } finally {
    if (current === generation) update(serverId, { busy: false });
  }
}

/** Keep daemon controls healthy after leaving Settings; account authority renews their leases. */
export function OnlineServiceHostSyncLifecycle() {
  const status = useAccountState((state) => state.status);
  const synchronizedHosts = useAccountState((state) => state.synchronizedHosts);
  useEffect(() => {
    if (status === "logged_out" && synchronizedHosts)
      reconcileOnlineServiceHostsAfterLogout(synchronizedHosts);
    else if (synchronizedHosts)
      for (const serverId of synchronizedHosts) update(serverId, { enabled: true });
  }, [status, synchronizedHosts]);
  useEffect(() => {
    const poll = () => {
      const store = getHostRuntimeStore();
      for (const [serverId, state] of Object.entries(useOnlineServiceHostSync.getState().hosts)) {
        if (!state.enabled) continue;
        const host = store.getHosts().find((host) => host.serverId === serverId);
        if (host) void synchronizeOnlineServiceHost(serverId, host.label);
      }
    };
    const timer = setInterval(poll, 5000);
    return () => clearInterval(timer);
  }, []);
  return null;
}

/** Invalidate in-flight UI work, preserving the remote leases retained by account authority. */
export function reconcileOnlineServiceHostsAfterLogout(serverIds: readonly string[]): void {
  generation += 1;
  useOnlineServiceHostSync.setState(({ hosts }) => ({
    hosts: Object.fromEntries(
      Object.entries(hosts).map(([serverId, state]) => [
        serverId,
        {
          ...state,
          busy: false,
          enabled: serverIds.includes(serverId),
          status: serverIds.includes(serverId) ? state.status : null,
        },
      ]),
    ),
  }));
  for (const serverId of serverIds) update(serverId, { enabled: true });
}

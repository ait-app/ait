import { useEffect } from "react";
import { Platform } from "react-native";
import { create } from "zustand";
import type { RelayStatus } from "@ait/protocol/relay";
import type { DeviceAuthorization, PublicationStatus } from "@ait/protocol/publication";
import { getDesktopHost } from "@/desktop/host";
import { getDesktopDaemonStatus } from "@/desktop/daemon/desktop-daemon";
import { getNativeAccount } from "./native-account";
import { getHostRuntimeStore } from "./host-runtime";
import { serializeNativeAccountCommand, useAccountState } from "./account-state";

interface HostSyncState {
  enabled: boolean;
  busy: boolean;
  status: RelayStatus | null;
  publication?: PublicationStatus;
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
): Promise<DeviceAuthorization | null> {
  const desktop = getDesktopHost();
  if (desktop?.invoke) return desktop.invoke(command, args) as Promise<DeviceAuthorization | null>;
  if (Platform.OS !== "android" && Platform.OS !== "ios")
    throw new Error("Online service synchronization requires the desktop or native mobile app.");
  return serializeNativeAccountCommand(async () => {
    const manager = await getNativeAccount();
    if (command === "account_host_disconnect") {
      return null;
    }
    return args.needsAuthorization === true
      ? manager.authorizeHost({
          serverId: args.serverId as string,
          instanceId: args.instanceId as string,
          name: args.name as string,
          platform: args.platform as string,
        })
      : null;
  });
}

// Retain the same issuance identity across a lost daemon response; never consume codes here.
const handoffs = new Map<string, DeviceAuthorization>();

function publicationState(publication: PublicationStatus): Partial<HostSyncState> {
  return {
    publication,
    enabled: ![
      "unauthorized",
      "disconnected",
      "reauthorization_required",
      "protocol_error",
      "storage_error",
    ].includes(publication.management?.phase ?? ""),
    status: {
      serverId: publication.serverId,
      instanceId: publication.instanceId,
      platform: publication.platform,
      management: publication.management,
      status: { ...publication.hub.status, epoch: null },
    },
  };
}

/** Observe daemon-owned publication; explicit sign-out requires a new authorization. */
export async function synchronizeOnlineServiceHost(
  serverId: string,
  name: string,
  enable = false,
  options?: { automatic?: boolean },
): Promise<void> {
  const previous = useOnlineServiceHostSync.getState().hosts[serverId] ?? EMPTY_STATE;
  if (previous.busy) return;
  const client = getHostRuntimeStore().getClient(serverId);
  if (!client?.isConnected) return;
  const current = generation;
  update(serverId, { busy: true, error: null, enabled: enable || previous.enabled });
  try {
    const status = await client.getOnlineServiceStatus();
    if (status.serverId !== serverId)
      throw new Error("The host identity has changed. Reconnect first.");
    if (current !== generation) return;
    if (!status.management?.autonomous) {
      update(serverId, { status, enabled: false });
      if (enable) throw new Error("Update the host to use daemon-owned publication.");
      return;
    }
    let publication = await client.getPublicationStatus();
    const phase = publication.management?.phase;
    if (phase === "disconnected") handoffs.delete(serverId);
    const needsAuthorization = [
      "unauthorized",
      "disconnected",
      "reauthorization_required",
      "protocol_error",
      "storage_error",
    ].includes(phase ?? "");
    if (enable && (!options?.automatic || phase === "unauthorized")) {
      if (needsAuthorization && useAccountState.getState().status === "logged_out") {
        update(serverId, publicationState(publication));
        return;
      }
      let authorization = handoffs.get(serverId);
      if (authorization && Date.parse(authorization.expires_at) <= Date.now()) {
        handoffs.delete(serverId);
        authorization = undefined;
      }
      const approved = await hostCommand("account_host_sync", {
        serverId,
        instanceId: status.instanceId,
        platform: status.platform,
        name,
        needsAuthorization: needsAuthorization && !authorization,
        ...(!options?.automatic ? { enableBuiltInDaemon: true } : {}),
      });
      if (current !== generation) return;
      authorization ??= approved ?? undefined;
      if (needsAuthorization && authorization) {
        handoffs.set(serverId, authorization);
        publication = await client.authorizePublication(
          authorization,
          options?.automatic ? "initial" : "explicit",
        );
        handoffs.delete(serverId);
      }
    }
    if (current === generation) update(serverId, publicationState(publication));
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
    const currentStatus = await client.getOnlineServiceStatus();
    if (currentStatus.serverId !== serverId)
      throw new Error("The host identity has changed. Reconnect first.");
    if (!currentStatus.management?.autonomous)
      throw new Error("Update the host to control its server connection.");
    handoffs.delete(serverId);
    await hostCommand("account_host_disconnect", { serverId });
    if (current !== generation) return;
    const publication = await client.disconnectPublication();
    if (current === generation) update(serverId, publicationState(publication));
  } catch (error) {
    if (current === generation)
      update(serverId, {
        error: error instanceof Error ? error.message : "Host synchronization failed.",
      });
  } finally {
    if (current === generation) update(serverId, { busy: false });
  }
}

/** Publish the built-in desktop daemon by default, respecting a persisted manual stop. */
export async function synchronizeDefaultDesktopHost(): Promise<void> {
  const account = useAccountState.getState();
  if (
    !getDesktopHost()?.invoke ||
    account.status !== "online" ||
    account.syncBuiltInDaemon === false
  )
    return;
  const daemon = await getDesktopDaemonStatus();
  // Account intent can change while the daemon status request is in flight.
  const current = useAccountState.getState();
  if (
    current.status !== "online" ||
    current.syncBuiltInDaemon === false ||
    daemon.status !== "running"
  )
    return;
  const store = getHostRuntimeStore();
  const host = store.getHosts().find((host) => host.serverId === daemon.serverId);
  const client = store.getClient(daemon.serverId);
  if (!host || client?.getLastServerInfoMessage()?.features?.onlineServiceSync !== true) return;
  await synchronizeOnlineServiceHost(host.serverId, host.label, true, { automatic: true });
}

/** Poll display state; node registration, renewal and reconnects belong to the daemon. */
export function OnlineServiceHostSyncLifecycle() {
  const status = useAccountState((state) => state.status);
  const synchronizedHosts = useAccountState((state) => state.synchronizedHosts);
  const syncBuiltInDaemon = useAccountState((state) => state.syncBuiltInDaemon);
  useEffect(() => {
    if (status === "logged_out" && synchronizedHosts)
      reconcileOnlineServiceHostsAfterLogout(synchronizedHosts);
    else if (synchronizedHosts)
      for (const serverId of synchronizedHosts) update(serverId, { enabled: true });
  }, [status, synchronizedHosts]);
  useEffect(() => {
    const poll = () => {
      void synchronizeDefaultDesktopHost().catch(() => undefined);
      const store = getHostRuntimeStore();
      for (const [serverId, state] of Object.entries(useOnlineServiceHostSync.getState().hosts)) {
        if (!state.enabled) continue;
        const host = store.getHosts().find((host) => host.serverId === serverId);
        if (host) void synchronizeOnlineServiceHost(serverId, host.label);
      }
    };
    poll();
    const timer = setInterval(poll, 5000);
    return () => clearInterval(timer);
  }, [status, syncBuiltInDaemon]);
  return null;
}

/** Invalidate account-dependent UI work without changing any daemon authorization or connection. */
export function reconcileOnlineServiceHostsAfterLogout(serverIds: readonly string[]): void {
  generation += 1;
  handoffs.clear();
  useOnlineServiceHostSync.setState(({ hosts }) => ({
    hosts: Object.fromEntries(
      Object.entries(hosts).map(([serverId, state]) => [
        serverId,
        {
          ...state,
          busy: false,
          enabled: state.publication ? state.enabled : serverIds.includes(serverId),
          status: state.publication || serverIds.includes(serverId) ? state.status : null,
        },
      ]),
    ),
  }));
  for (const serverId of serverIds) update(serverId, { enabled: true });
}

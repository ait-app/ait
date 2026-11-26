import { useMemo } from "react";
import type { ShareSetting } from "@ait/protocol/messages";
import type { HubAccountSnapshot } from "@ait/client/internal/hub";
import { useFetchQueries } from "@/data/query";
import { queryClient } from "@/data/query-client";
import {
  getHostRuntimeStore,
  isSharedResourceHost,
  useHostRuntimeConnectionStatuses,
  useHosts,
} from "@/runtime/host-runtime";
import { useHubServices } from "@/runtime/hub-service-store";

export interface OutgoingShare {
  serverId: string;
  hostLabel: string;
  groupName: string | null;
  setting: ShareSetting;
}

/** One Host's answer. The Host label is joined at read time because it can be renamed. */
interface HostShare {
  groupName: string | null;
  setting: ShareSetting;
}

export interface OutgoingShares {
  /** Shares of the Hosts that are connected right now. */
  shares: OutgoingShare[];
  loading: boolean;
  error: Error | null;
}

const EMPTY_INCOMING_SHARES = [] as const;
const OUTGOING_SHARES_KEY = ["outgoing-shares"] as const;

function outgoingSharesKey(serverId: string) {
  return [...OUTGOING_SHARES_KEY, serverId] as const;
}

export function selectIncomingShares(snapshot: Pick<HubAccountSnapshot, "shares"> | null) {
  return snapshot?.shares ?? EMPTY_INCOMING_SHARES;
}

async function fetchHostShares(serverId: string): Promise<HostShare[]> {
  const client = getHostRuntimeStore().getClient(serverId);
  if (!client) return [];
  const result = await client.listShares().catch((cause: unknown) => {
    const code = cause && typeof cause === "object" && "code" in cause ? cause.code : null;
    if (code === "not_implemented") return null;
    throw cause;
  });
  if (!result) return [];
  const groupNames = new Map(result.groups.map((group) => [group.groupId, group.name]));
  return result.shares
    .filter((setting) => setting.state !== "revoked")
    .map((setting) => ({
      groupName: setting.groupId ? (groupNames.get(setting.groupId) ?? setting.groupId) : null,
      setting,
    }));
}

/**
 * What the user shares from their own Hosts. Each connected Host is asked separately, so one
 * unreachable daemon does not hide the shares of every other Host.
 */
export function useOutgoingShares(): OutgoingShares {
  const hosts = useHosts();
  const ownerHosts = useMemo(() => hosts.filter((host) => !isSharedResourceHost(host)), [hosts]);
  const serverIds = useMemo(() => ownerHosts.map((host) => host.serverId), [ownerHosts]);
  const statuses = useHostRuntimeConnectionStatuses(serverIds);
  const results = useFetchQueries(
    ownerHosts.map((host) => ({
      queryKey: outgoingSharesKey(host.serverId),
      queryFn: () => fetchHostShares(host.serverId),
      enabled: statuses.get(host.serverId) === "online",
      dataShape: "list" as const,
      staleTimeMs: 0,
      retry: false,
    })),
  );

  const shares: OutgoingShare[] = [];
  let loading = false;
  let error: Error | null = null;
  ownerHosts.forEach((host, index) => {
    const result = results[index];
    // A disconnected Host keeps its last answer in the cache; that answer is not current.
    if (!result || statuses.get(host.serverId) !== "online") return;
    loading ||= result.isFetching;
    error ??= result.error;
    for (const share of result.data ?? []) {
      shares.push({ serverId: host.serverId, hostLabel: host.label, ...share });
    }
  });
  return { shares, loading, error };
}

/** True when either share list has an entry: shared with the user, or shared by the user. */
export function useHasAnyShares(): boolean {
  const hasIncoming = useHubServices((state) => selectIncomingShares(state.snapshot).length > 0);
  const hasOutgoing = useOutgoingShares().shares.length > 0;
  return hasIncoming || hasOutgoing;
}

/** Asks connected Hosts again. Pass a Host to limit the reload to it. */
export function refreshOutgoingShares(serverId?: string): void {
  void queryClient.invalidateQueries({
    queryKey: serverId ? outgoingSharesKey(serverId) : OUTGOING_SHARES_KEY,
  });
}

/** Drops a share the Host confirmed as removed, without waiting for another list request. */
export async function forgetOutgoingShare(serverId: string, settingId: string): Promise<void> {
  const queryKey = outgoingSharesKey(serverId);
  // A list request started before the removal must not restore the stopped share.
  await queryClient.cancelQueries({ queryKey });
  queryClient.setQueryData<HostShare[]>(queryKey, (current) =>
    current?.filter((share) => share.setting.settingId !== settingId),
  );
}

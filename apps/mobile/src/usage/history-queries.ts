import type { UsageLedgerGroup } from "@ait/protocol/messages";
import { useCallback, useMemo } from "react";
import { useQueryClient, type UseQueryResult } from "@tanstack/react-query";
import { useFetchQueries } from "@/data/query";
import { groupHosts } from "@/hosts/host-groups";
import { useHostFeatureAvailabilityMap } from "@/runtime/host-features";
import {
  getHostRuntimeStore,
  useHostRuntimeConnectionStatuses,
  useHosts,
} from "@/runtime/host-runtime";
import { usageCopy } from "./copy";
import {
  combineUsageHistory,
  historyWindow,
  isLedgerUnsupported,
  type HistoryRange,
  type LedgerAnswer,
  type LedgerQueryState,
  type UsageHistory,
} from "./history-model";

// Summaries cover a range ending when they are fetched; a minute later they are still close enough.
const LEDGER_STALE_TIME_MS = 60_000;
const MAX_LEDGER_RETRIES = 2;

/** The two summaries History reads per host and range: provider and model rows, and day rows. */
type LedgerGrouping = "model" | "day";
const GROUP_BY: Record<LedgerGrouping, UsageLedgerGroup[]> = {
  model: ["provider", "model"],
  day: ["day"],
};

/** Every ledger summary of a host. */
function hostLedgerQueryKey(serverId: string) {
  return ["usageLedger", serverId] as const;
}

function ledgerQueryKey(serverId: string, range: HistoryRange, grouping: LedgerGrouping) {
  return [...hostLedgerQueryKey(serverId), range, grouping] as const;
}

async function loadLedger(
  serverId: string,
  range: HistoryRange,
  grouping: LedgerGrouping,
): Promise<LedgerAnswer> {
  const client = getHostRuntimeStore().getClient(serverId);
  if (!client) throw new Error(usageCopy.clientUnavailable);
  const window = historyWindow(range, Date.now());
  const summary = await client.listUsageLedger({ ...window, groupBy: GROUP_BY[grouping] });
  return { ...summary, from: window.from };
}

/** A host without a ledger answers the same way every time; retrying would only delay saying so. */
function retryLedger(failureCount: number, error: Error): boolean {
  return !isLedgerUnsupported(error) && failureCount < MAX_LEDGER_RETRIES;
}

function ledgerQuery(
  host: { serverId: string; isConnected: boolean; supportsLedger: boolean | null },
  range: HistoryRange,
  grouping: LedgerGrouping,
) {
  return {
    queryKey: ledgerQueryKey(host.serverId, range, grouping),
    queryFn: () => loadLedger(host.serverId, range, grouping),
    enabled: host.isConnected && host.supportsLedger === true,
    // Another range's figures never stand in while this one loads.
    dataShape: "value" as const,
    retry: retryLedger,
    staleTimeMs: LEDGER_STALE_TIME_MS,
  };
}

function toLedgerState(query: UseQueryResult<LedgerAnswer, Error> | undefined): LedgerQueryState {
  return { data: query?.data, error: query?.error ?? null, isFetching: query?.isFetching ?? false };
}

export interface UsageHistoryHost {
  serverId: string;
  label: string;
  isConnected: boolean;
  supportsLedger: boolean | null;
}

/** Every host, one per daemon, with whether it is connected and offers the ledger. */
export function useUsageHistoryHosts(): UsageHistoryHost[] {
  const hostProfiles = useHosts();
  const hosts = useMemo(() => groupHosts(hostProfiles), [hostProfiles]);
  const serverIds = useMemo(() => hosts.map((host) => host.serverId), [hosts]);
  const statuses = useHostRuntimeConnectionStatuses(serverIds);
  const support = useHostFeatureAvailabilityMap(serverIds, "providerUsageLedger");
  return useMemo(
    () =>
      hosts.map((host) => ({
        serverId: host.serverId,
        label: host.label,
        isConnected: statuses.get(host.serverId) === "online",
        supportsLedger: support.get(host.serverId) ?? null,
      })),
    [hosts, statuses, support],
  );
}

/**
 * One host's History for a range. The host answers for itself; while it cannot, the History lists
 * it rather than counting it as zero.
 */
export function useUsageHistory(
  serverId: string,
  range: HistoryRange,
): { history: UsageHistory; refresh: () => void } {
  const queryClient = useQueryClient();
  const allHosts = useUsageHistoryHosts();
  const hosts = useMemo(
    () => allHosts.filter((host) => host.serverId === serverId),
    [allHosts, serverId],
  );
  const queries = useFetchQueries<LedgerAnswer>(
    hosts.flatMap((host) => [ledgerQuery(host, range, "model"), ledgerQuery(host, range, "day")]),
  );
  const history = combineUsageHistory(
    hosts.map((host, index) => ({
      ...host,
      byModel: toLedgerState(queries[index * 2]),
      byDay: toLedgerState(queries[index * 2 + 1]),
    })),
  );
  const refresh = useCallback(() => {
    const runtime = getHostRuntimeStore();
    for (const host of hosts) {
      if (!host.isConnected) {
        runtime.retryConnection(host.serverId);
        continue;
      }
      void queryClient.invalidateQueries({ queryKey: hostLedgerQueryKey(host.serverId) });
    }
  }, [hosts, queryClient]);
  return { history, refresh };
}

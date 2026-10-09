import { toUsageReport } from "./native-report";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { useCallback, useMemo } from "react";
import {
  skipToken,
  useMutation,
  useQueryClient,
  type QueryClient,
  type QueryKey,
} from "@tanstack/react-query";
import { useShallow } from "zustand/shallow";
import { useFetchQuery } from "@/data/query";
import {
  getHostRuntimeStore,
  useHostRuntimeConnectionStatuses,
  useHostRuntimeIsConnected,
  useHosts,
} from "@/runtime/host-runtime";
import { useSessionStore, type SessionState } from "@/stores/session-store";
import { usageCopy } from "./copy";
import {
  replaceReport,
  resolveAgentUsageView,
  resolveUsageRefresh,
  resolveUsageView,
  settleReports,
  type UsageHost,
  type UsageQueryState,
  type AgentUsageView,
  type UsageRefresh,
  upsertReport,
} from "./model";
import type { UsageReportEntry, UsageView } from "./types";

// Host reports use the daemon's five-minute cache; explicit refresh bypasses it.
// Agent reports always read the live session's account and stay separate from host reports.
const REPORTS_STALE_TIME_MS = 60_000;

/** Every report list of a host: its own and each agent's. */
function hostUsageQueryKey(serverId: string) {
  return ["usage", serverId] as const;
}

function usageReportsQueryKey(serverId: string) {
  return [...hostUsageQueryKey(serverId), "reports"] as const;
}

function agentUsageQueryKey(serverId: string, agentId: string) {
  return [...hostUsageQueryKey(serverId), "agent", agentId] as const;
}

function requireClient(serverId: string) {
  const client = getHostRuntimeStore().getClient(serverId);
  if (!client) throw new Error(usageCopy.clientUnavailable);
  return client;
}

/**
 * Loads a host's reports or one agent's. AIT returns one native-provider snapshot;
 * settling it preserves any newer report refreshed while the request was in flight.
 */
async function loadReports(input: {
  queryClient: QueryClient;
  queryKey: QueryKey;
  serverId: string;
  agentId?: string;
  forceRefresh?: boolean;
  /** Once aborted, reports still on their way are dropped instead of written. */
  signal?: AbortSignal;
}): Promise<UsageReportEntry[]> {
  const { queryClient, queryKey, serverId, agentId, forceRefresh = false, signal } = input;
  const { reports } = await listUsageReports(
    requireClient(serverId),
    { agentId, forceRefresh },
    (report) => {
      if (signal?.aborted) return;
      queryClient.setQueryData<UsageReportEntry[]>(queryKey, (current) =>
        upsertReport(current, report),
      );
    },
  );
  return settleReports(queryClient.getQueryData<UsageReportEntry[]>(queryKey), reports);
}

function listReports(
  queryClient: QueryClient,
  serverId: string,
  forceRefresh = false,
): Promise<UsageReportEntry[]> {
  return loadReports({
    queryClient,
    queryKey: usageReportsQueryKey(serverId),
    serverId,
    forceRefresh,
  });
}

async function getReport(
  serverId: string,
  reportId: string,
  forceRefresh = false,
  agentId?: string,
): Promise<UsageReportEntry | null> {
  return (
    (
      await listUsageReports(
        requireClient(serverId),
        agentId === undefined ? { reportIds: [reportId], forceRefresh } : { agentId, forceRefresh },
      )
    ).reports.find((report) => report.id === reportId) ?? null
  );
}

/** Adapt AIT's native-provider quota boundary to the shared Usage presentation. */
async function listUsageReports(
  client: DaemonClient,
  options: { agentId?: string; reportIds?: string[]; forceRefresh?: boolean },
  onReport?: (report: UsageReportEntry) => void,
): Promise<{ reports: UsageReportEntry[] }> {
  const result = await client.listProviderUsage({
    agentId: options.agentId,
    forceRefresh: options.forceRefresh,
    ...(options.agentId === undefined && options.reportIds?.length === 1
      ? { providerId: options.reportIds[0] }
      : {}),
  });
  const reports = result.providers
    .map((provider) => toUsageReport(provider, result.fetchedAt))
    .filter((report) => !options.reportIds || options.reportIds.includes(report.id));
  reports.forEach((report) => onReport?.(report));
  return { reports };
}

function supportsUsage(session: SessionState | undefined): boolean {
  return session?.serverInfo?.features?.providerUsageList === true;
}

async function refreshReports(queryClient: QueryClient, serverId: string): Promise<void> {
  await queryClient.fetchQuery({
    queryKey: usageReportsQueryKey(serverId),
    queryFn: () => listReports(queryClient, serverId, true),
    staleTime: 0,
  });
}

function toQueryState(query: {
  data: UsageReportEntry[] | undefined;
  error: unknown;
  isFetching: boolean;
}): UsageQueryState {
  return { data: query.data, error: query.error, isFetching: query.isFetching };
}

/** Usage reports for one host, as shown on its settings page. */
export function useHostUsage(serverId: string): { view: UsageView; refresh: () => void } {
  const queryClient = useQueryClient();
  const isConnected = useHostRuntimeIsConnected(serverId);
  const isSupported = useSessionStore((state) => supportsUsage(state.sessions[serverId]));
  const query = useFetchQuery({
    queryKey: usageReportsQueryKey(serverId),
    queryFn: () => listReports(queryClient, serverId),
    enabled: isConnected && isSupported,
    dataShape: "list",
    staleTimeMs: REPORTS_STALE_TIME_MS,
  });
  const refresh = useCallback(() => {
    void refreshReports(queryClient, serverId).catch(() => undefined);
  }, [queryClient, serverId]);
  const hostLabel = useHosts().find((host) => host.serverId === serverId)?.label ?? serverId;
  const view = resolveUsageView({
    hostLabel,
    isConnected,
    supportsUsage: isSupported,
    query: toQueryState(query),
  });
  return { view, refresh };
}

const NO_REPORTS: UsageReportEntry[] = [];

/**
 * The reports of the sidebar's usage host, which is connected and reports usage; none until they
 * load, or without a host.
 */
export function useUsageHostReports(serverId: string | null): UsageReportEntry[] {
  const queryClient = useQueryClient();
  const query = useFetchQuery({
    queryKey: usageReportsQueryKey(serverId ?? ""),
    queryFn: serverId ? () => listReports(queryClient, serverId) : skipToken,
    dataShape: "list",
    staleTimeMs: REPORTS_STALE_TIME_MS,
  });
  return query.data ?? NO_REPORTS;
}

/**
 * The reports of the account an agent runs under, as its meter's tooltip or sheet shows them.
 * Fetched while either is mounted, so only while it is open.
 */
export function useAgentUsage(serverId: string, agentId: string): AgentUsageView {
  const queryClient = useQueryClient();
  const canReport = useHostReportsUsage(serverId);
  const queryKey = agentUsageQueryKey(serverId, agentId);
  const query = useFetchQuery({
    queryKey,
    // Consuming the signal cancels the request when the details close, so a reopen sends a new
    // request instead of joining one scoped to the login the agent ran under before.
    queryFn: ({ signal }) => loadReports({ queryClient, queryKey, serverId, agentId, signal }),
    enabled: canReport,
    // Another agent's reports never stand in while this one's load.
    dataShape: "value",
    // Dropped as soon as the details close: an agent resumed under another login keeps its ID, so
    // reports kept from an earlier open could show the old login. Each open reads the live account.
    gcTime: 0,
    // The daemon's errors (an unknown agent) do not heal on retry, and reopening the details
    // fetches again; retrying would hold the loading sentence for seconds instead.
    retry: false,
    staleTimeMs: REPORTS_STALE_TIME_MS,
  });
  return resolveAgentUsageView({ canReport, query: toQueryState(query) });
}

/** Whether a host is connected and reports usage, read without fetching anything. */
export function useHostReportsUsage(serverId: string): boolean {
  const isConnected = useHostRuntimeIsConnected(serverId);
  const isSupported = useSessionStore((state) => supportsUsage(state.sessions[serverId]));
  return isConnected && isSupported;
}

/** Every host with whether it is connected and reports usage, in host order. */
export function useUsageHosts(): UsageHost[] {
  const hosts = useHosts();
  const serverIds = useMemo(() => hosts.map((host) => host.serverId), [hosts]);
  const connectionStatuses = useHostRuntimeConnectionStatuses(serverIds);
  const supportedServerIds = useSessionStore(
    useShallow((state) => serverIds.filter((serverId) => supportsUsage(state.sessions[serverId]))),
  );
  return useMemo(
    () =>
      hosts.map((host) => ({
        serverId: host.serverId,
        label: host.label,
        isConnected: connectionStatuses.get(host.serverId) === "online",
        supportsUsage: supportedServerIds.includes(host.serverId),
      })),
    [connectionStatuses, hosts, supportedServerIds],
  );
}

/**
 * Refreshes a host report, or the agent's session reports. The result replaces the card
 * in the requesting view. Host and agent views can use different logins of the same account.
 */
export function useReportRefresh(
  serverId: string,
  reportId: string,
  agentId?: string,
): { refresh: () => void; refreshState: UsageRefresh } {
  const queryClient = useQueryClient();
  const mutation = useMutation({
    mutationFn: () => getReport(serverId, reportId, true, agentId),
    onSuccess: (report) => {
      queryClient.setQueryData<UsageReportEntry[]>(
        agentId === undefined
          ? usageReportsQueryKey(serverId)
          : agentUsageQueryKey(serverId, agentId),
        (reports) => (reports ? replaceReport(reports, reportId, report) : reports),
      );
    },
  });
  const { mutate } = mutation;
  const refresh = useCallback(() => mutate(), [mutate]);
  return { refresh, refreshState: resolveUsageRefresh(mutation) };
}

import { getHostRuntimeStore } from "@/runtime/host-runtime";
import { useCallback, useMemo } from "react";
import {
  useMutation,
  useQuery,
  useQueryClient,
  type QueryClient,
  type QueryKey,
} from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { useSessionStore } from "@/stores/session-store";
import {
  ARCHIVE_AGENT_PENDING_QUERY_KEY,
  clearArchiveAgentPending,
  selectPendingArchiveAgentIds,
  setAgentArchiving,
  toArchiveKey,
  type ArchiveAgentInput,
  type ArchiveAgentPendingState,
} from "@/data/archive-agent-pending";
import { agentHistoryQueryKey, allAgentHistoryQueryRootKey } from "./agent-history-query-key";

interface AgentsListQueryData {
  entries?: Array<{ agent?: { id?: string | null } | null } | null>;
}

interface AgentHistoryQueryAgent {
  id?: string | null;
  serverId?: string | null;
  archivedAt?: Date | null;
}

interface AgentHistoryQueryPage {
  agents?: AgentHistoryQueryAgent[];
}

interface AgentHistoryQueryData {
  pages?: AgentHistoryQueryPage[];
}

export function removeAgentFromListPayload<T extends AgentsListQueryData | undefined>(
  payload: T,
  agentId: string,
): T {
  if (!payload || !Array.isArray(payload.entries) || !agentId) {
    return payload;
  }
  const filtered = payload.entries.filter((entry) => entry?.agent?.id !== agentId);
  if (filtered.length === payload.entries.length) {
    return payload;
  }
  return {
    ...payload,
    entries: filtered,
  } as T;
}

function removeAgentFromCachedLists(queryClient: QueryClient, input: ArchiveAgentInput): void {
  const agentId = input.agentId.trim();
  if (!agentId) {
    return;
  }

  queryClient.setQueryData<AgentsListQueryData | undefined>(
    ["sidebarAgentsList", input.serverId],
    (current) => removeAgentFromListPayload(current, agentId),
  );
  queryClient.setQueryData<AgentsListQueryData | undefined>(
    ["allAgents", input.serverId],
    (current) => removeAgentFromListPayload(current, agentId),
  );
}

function markAgentArchivedInHistoryPayload<T extends AgentHistoryQueryData | undefined>(
  payload: T,
  input: ArchiveAgentInput & { archivedAt: string },
): T {
  if (!payload || !Array.isArray(payload.pages) || !input.agentId) {
    return payload;
  }

  const archivedAt = new Date(input.archivedAt);
  if (Number.isNaN(archivedAt.getTime())) {
    return payload;
  }

  let changed = false;
  const pages = payload.pages.map((page) => {
    if (!Array.isArray(page.agents)) {
      return page;
    }

    let pageChanged = false;
    const agents = page.agents.map((agent) => {
      if (
        agent.id !== input.agentId ||
        (agent.serverId != null && agent.serverId !== input.serverId)
      ) {
        return agent;
      }
      pageChanged = true;
      changed = true;
      return {
        ...agent,
        archivedAt,
      };
    });

    return pageChanged ? { ...page, agents } : page;
  });

  return changed ? ({ ...payload, pages } as T) : payload;
}

function markAgentArchivedInHistoryCache(
  queryClient: QueryClient,
  input: ArchiveAgentInput & { archivedAt: string },
): void {
  queryClient.setQueryData<AgentHistoryQueryData | undefined>(
    agentHistoryQueryKey(input.serverId),
    (current) => markAgentArchivedInHistoryPayload(current, input),
  );
  queryClient.setQueriesData<AgentHistoryQueryData | undefined>(
    { queryKey: allAgentHistoryQueryRootKey() },
    (current) => markAgentArchivedInHistoryPayload(current, input),
  );
}

interface ArchivedAgentCloseResult {
  agentId: string;
  archivedAt: string;
}

interface ArchivedAgentListCacheSnapshot {
  sidebarAgentsList: AgentsListQueryData | undefined;
  allAgents: AgentsListQueryData | undefined;
  agentHistory: AgentHistoryQueryData | undefined;
  allAgentHistory: Array<[QueryKey, AgentHistoryQueryData | undefined]>;
}

interface ArchiveAgentMutationContext {
  agent: ReturnType<typeof getStoredAgentSnapshot>;
  lists: ArchivedAgentListCacheSnapshot;
}

function getStoredAgentSnapshot(input: ArchiveAgentInput) {
  return useSessionStore.getState().sessions[input.serverId]?.agents.get(input.agentId);
}

function restoreAgentSnapshot(
  input: ArchiveAgentInput & { agent: ReturnType<typeof getStoredAgentSnapshot> },
): void {
  getHostRuntimeStore().restoreAgentSnapshot(input.serverId, input.agentId, input.agent);
}

function getArchivedAgentListCacheSnapshot(
  queryClient: QueryClient,
  serverId: string,
): ArchivedAgentListCacheSnapshot {
  return {
    sidebarAgentsList: queryClient.getQueryData<AgentsListQueryData | undefined>([
      "sidebarAgentsList",
      serverId,
    ]),
    allAgents: queryClient.getQueryData<AgentsListQueryData | undefined>(["allAgents", serverId]),
    agentHistory: queryClient.getQueryData<AgentHistoryQueryData | undefined>(
      agentHistoryQueryKey(serverId),
    ),
    allAgentHistory: queryClient.getQueriesData<AgentHistoryQueryData | undefined>({
      queryKey: allAgentHistoryQueryRootKey(),
    }),
  };
}

function restoreCachedQuerySnapshot(
  queryClient: QueryClient,
  queryKey: readonly unknown[],
  snapshot: unknown,
): void {
  if (snapshot === undefined) {
    queryClient.removeQueries({ queryKey, exact: true });
    return;
  }
  queryClient.setQueryData(queryKey, snapshot);
}

function restoreArchivedAgentListCacheSnapshot(
  queryClient: QueryClient,
  serverId: string,
  snapshot: ArchivedAgentListCacheSnapshot,
): void {
  restoreCachedQuerySnapshot(
    queryClient,
    ["sidebarAgentsList", serverId],
    snapshot.sidebarAgentsList,
  );
  restoreCachedQuerySnapshot(queryClient, ["allAgents", serverId], snapshot.allAgents);
  restoreCachedQuerySnapshot(queryClient, agentHistoryQueryKey(serverId), snapshot.agentHistory);
  for (const [queryKey, querySnapshot] of snapshot.allAgentHistory) {
    restoreCachedQuerySnapshot(queryClient, queryKey, querySnapshot);
  }
}

function markAgentArchivedInStore(input: ArchiveAgentInput & { archivedAt: string }): void {
  const archivedAt = new Date(input.archivedAt);
  if (Number.isNaN(archivedAt.getTime())) {
    return;
  }

  getHostRuntimeStore().archiveAgentSnapshot(
    input.serverId,
    input.agentId,
    archivedAt.toISOString(),
  );
}

interface ApplyArchivedAgentCloseResultsInput {
  queryClient: QueryClient;
  serverId: string;
  results: ArchivedAgentCloseResult[];
}

export function applyArchivedAgentCloseResults(input: ApplyArchivedAgentCloseResultsInput): void {
  for (const result of input.results) {
    markAgentArchivedInStore({
      serverId: input.serverId,
      agentId: result.agentId,
      archivedAt: result.archivedAt,
    });
    removeAgentFromCachedLists(input.queryClient, {
      serverId: input.serverId,
      agentId: result.agentId,
    });
    markAgentArchivedInHistoryCache(input.queryClient, {
      serverId: input.serverId,
      agentId: result.agentId,
      archivedAt: result.archivedAt,
    });
  }
}

function useArchiveAgentPendingQuery() {
  return useQuery({
    queryKey: ARCHIVE_AGENT_PENDING_QUERY_KEY,
    queryFn: async (): Promise<ArchiveAgentPendingState> => ({}),
    initialData: {} as ArchiveAgentPendingState,
    staleTime: Infinity,
    gcTime: Infinity,
  });
}

export function usePendingArchiveAgentIds(serverId: string): ReadonlySet<string> {
  const pendingQuery = useArchiveAgentPendingQuery();
  return useMemo(
    () => selectPendingArchiveAgentIds(pendingQuery.data ?? {}, serverId),
    [pendingQuery.data, serverId],
  );
}

export function useArchiveAgent() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  const pendingQuery = useArchiveAgentPendingQuery();

  const archiveMutation = useMutation({
    mutationFn: async (input: ArchiveAgentInput): Promise<{ archivedAt: string }> => {
      const client = useSessionStore.getState().sessions[input.serverId]?.client ?? null;
      if (!client) {
        throw new Error(t("common.errors.daemonClientUnavailable"));
      }
      return await client.archiveAgent(input.agentId);
    },
    onMutate: (input) => {
      const context: ArchiveAgentMutationContext = {
        agent: getStoredAgentSnapshot(input),
        lists: getArchivedAgentListCacheSnapshot(queryClient, input.serverId),
      };
      const archivedAt = new Date().toISOString();

      applyArchivedAgentCloseResults({
        queryClient,
        serverId: input.serverId,
        results: [{ agentId: input.agentId, archivedAt }],
      });
      setAgentArchiving({
        queryClient,
        serverId: input.serverId,
        agentId: input.agentId,
        isArchiving: true,
      });
      return context;
    },
    onSuccess: (result, input) => {
      markAgentArchivedInStore({
        serverId: input.serverId,
        agentId: input.agentId,
        archivedAt: result.archivedAt,
      });
    },
    onError: (_error, input, context) => {
      if (!context) {
        return;
      }
      restoreAgentSnapshot({
        serverId: input.serverId,
        agentId: input.agentId,
        agent: context.agent,
      });
      restoreArchivedAgentListCacheSnapshot(queryClient, input.serverId, context.lists);
    },
    onSettled: (_result, _error, input) => {
      clearArchiveAgentPending({
        queryClient,
        serverId: input.serverId,
        agentId: input.agentId,
      });
      void queryClient.invalidateQueries({
        queryKey: ["sidebarAgentsList", input.serverId],
      });
      void queryClient.invalidateQueries({
        queryKey: ["allAgents", input.serverId],
      });
      void queryClient.invalidateQueries({
        queryKey: agentHistoryQueryKey(input.serverId),
      });
      void queryClient.invalidateQueries({
        queryKey: allAgentHistoryQueryRootKey(),
      });
    },
  });

  const archiveMutateAsync = archiveMutation.mutateAsync;

  const archiveAgent = useCallback(
    async (input: ArchiveAgentInput): Promise<void> => {
      await archiveMutateAsync(input);
    },
    [archiveMutateAsync],
  );

  const isArchivingAgent = useCallback(
    (input: ArchiveAgentInput): boolean => {
      const key = toArchiveKey(input);
      if (!key) {
        return false;
      }
      return (pendingQuery.data ?? {})[key] ?? false;
    },
    [pendingQuery.data],
  );

  return {
    archiveAgent,
    isArchivingAgent,
  };
}

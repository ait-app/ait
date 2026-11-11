import { useCallback } from "react";
import { useQuery, useQueryClient, type QueryClient, type QueryKey } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { useRetainedPanelActive } from "@/components/retained-panel";
import { useHostRuntimeClient, useHostRuntimeIsConnected } from "@/runtime/host-runtime";
import { agentCommandsQueryKey, type AgentCommandsDraftConfig } from "@/hooks/agent-commands-query";
import { useAgentCommandContext } from "./use-agent-command-context";

const DRAFT_COMMANDS_STALE_TIME = Number.POSITIVE_INFINITY;
const SESSION_COMMANDS_STALE_TIME = 60_000;

export interface AgentSlashCommand {
  name: string;
  description: string;
  argumentHint: string;
  kind?: string;
  /** Native provenance (builtin/plugin/skill/template/project); absent on older hosts. */
  source?: string;
  aliases?: string[];
  /** Ownership is retained even when the provider excludes this entry from menus. */
  menuVisible?: boolean;
  /** Native aliases eligible for menu matching; ownership still uses all aliases. */
  menuAliases?: string[];
}

/** Read ownership at action time; invalidated or loading data never authorizes a shortcut. */
export function readAgentCommandOwnership(queryClient: QueryClient, key: QueryKey) {
  const state = queryClient.getQueryState<AgentSlashCommand[]>(key);
  return state?.status === "success" && state.fetchStatus === "idle" && !state.isInvalidated
    ? state.data
    : undefined;
}

export type DraftCommandConfig = AgentCommandsDraftConfig;

interface ListAgentCommandsOptions {
  agentId: string;
  draftConfig?: DraftCommandConfig;
}

export interface AgentCommandsClient {
  listCommands(options: ListAgentCommandsOptions): ReturnType<DaemonClient["listCommands"]>;
}

export async function fetchAgentCommands(input: {
  client: AgentCommandsClient;
  agentId: string;
  draftConfig?: DraftCommandConfig;
}): Promise<AgentSlashCommand[]> {
  const response = await input.client.listCommands({
    agentId: input.agentId,
    draftConfig: input.draftConfig,
  });
  if (response.error) throw new Error(response.error);
  return response.commands as AgentSlashCommand[];
}

interface UseAgentCommandsQueryOptions {
  serverId: string;
  agentId: string;
  enabled?: boolean;
  draftConfig?: DraftCommandConfig;
}

export function useAgentCommandsQuery({
  serverId,
  agentId,
  enabled = true,
  draftConfig,
}: UseAgentCommandsQueryOptions) {
  const { t } = useTranslation();
  const retainedPanelActive = useRetainedPanelActive();
  const queryEnabled = enabled && retainedPanelActive;
  const client = useHostRuntimeClient(serverId);
  const isConnected = useHostRuntimeIsConnected(serverId);
  const context = useAgentCommandContext(serverId, agentId);
  const queryClient = useQueryClient();
  const queryKey = agentCommandsQueryKey({ serverId, agentId, draftConfig, context, client });
  const readOwnership = useCallback(
    () => readAgentCommandOwnership(queryClient, queryKey),
    [queryClient, queryKey],
  );

  const query = useQuery({
    queryKey,
    queryFn: async ({ signal }) => {
      if (!client) {
        throw new Error(t("common.errors.daemonClientUnavailable"));
      }
      const commands = await fetchAgentCommands({ client, agentId, draftConfig });
      signal.throwIfAborted();
      return commands;
    },
    enabled: queryEnabled && !!client && isConnected && (!!agentId || !!draftConfig),
    staleTime: draftConfig ? DRAFT_COMMANDS_STALE_TIME : SESSION_COMMANDS_STALE_TIME,
    retry: 3,
    retryDelay: (attemptIndex) => Math.min(1000 * 2 ** attemptIndex, 5000),
  });

  // Invalidated ownership is unavailable until the current-context refresh finishes.
  const isLoading = query.isPending || query.isFetching;
  const ownership =
    query.isSuccess && !query.isFetching && !queryClient.getQueryState(queryKey)?.isInvalidated
      ? query.data
      : undefined;

  return {
    commands: ownership ?? [],
    ownership,
    readOwnership,
    isLoading,
    isError: query.isError,
    error: query.error,
  };
}

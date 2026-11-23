import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { useRetainedPanelActive } from "@/hooks/use-retained-panel-active";
import { useHostRuntimeClient, useHostRuntimeIsConnected } from "@/runtime/host-runtime";
import { useSessionStore } from "@/stores/session-store";
import { agentCommandsQueryKey, type AgentCommandsDraftConfig } from "@/data/agent-commands-query";
import type { CommandOwnership } from "@/client-slash-commands";

const DRAFT_COMMANDS_STALE_TIME = Number.POSITIVE_INFINITY;
const SESSION_COMMANDS_STALE_TIME = 60_000;
const connectionIds = new WeakMap<object, number>();
let nextConnectionId = 0;

function connectionId(client: object | null): number | null {
  if (!client) return null;
  let id = connectionIds.get(client);
  if (id === undefined) {
    id = ++nextConnectionId;
    connectionIds.set(client, id);
  }
  return id;
}

export interface AgentSlashCommand {
  name: string;
  description: string;
  argumentHint: string;
  kind?: string;
  source?: string;
  aliases?: string[];
  menuVisible?: boolean;
  menuAliases?: string[];
}

export interface AgentCommandCatalog {
  commands: AgentSlashCommand[];
  ownershipComplete: boolean;
  ownershipApplicable?: boolean;
}

export function commandOwnershipForCatalog(
  supportsOwnership: boolean | undefined,
  catalog: AgentCommandCatalog | undefined,
): CommandOwnership {
  if (supportsOwnership === undefined) return { kind: "unavailable" };
  if (!supportsOwnership) return { kind: "legacy" };
  if (catalog?.ownershipApplicable === false) return { kind: "legacy" };
  return catalog?.ownershipApplicable === true && catalog.ownershipComplete
    ? { kind: "complete", commands: catalog.commands }
    : { kind: "unavailable" };
}

export type DraftCommandConfig = AgentCommandsDraftConfig;

interface ListAgentCommandsOptions {
  agentId: string;
  draftConfig?: DraftCommandConfig;
}

export interface AgentCommandsClient {
  listCommands(options: ListAgentCommandsOptions): ReturnType<DaemonClient["listCommands"]>;
}

/** The v2 host could not read the current session's native catalog. */
export class AgentCommandSessionCatalogUnavailable extends Error {}

export async function fetchAgentCommands(input: {
  client: AgentCommandsClient;
  agentId: string;
  draftConfig?: DraftCommandConfig;
}): Promise<AgentCommandCatalog> {
  const response = await input.client.listCommands({
    agentId: input.agentId,
    draftConfig: input.draftConfig,
  });
  if (response.error) {
    if (
      input.agentId &&
      !input.draftConfig &&
      response.ownershipApplicable === true &&
      response.ownershipComplete === false &&
      response.error === "Command catalog is unavailable"
    ) {
      throw new AgentCommandSessionCatalogUnavailable(response.error);
    }
    throw new Error(response.error);
  }
  return {
    commands: response.commands as AgentSlashCommand[],
    ownershipComplete:
      (response as typeof response & { ownershipComplete?: boolean }).ownershipComplete === true,
    ownershipApplicable: (response as typeof response & { ownershipApplicable?: boolean })
      .ownershipApplicable,
  };
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
  const provider = useSessionStore(
    (state) => state.sessions[serverId]?.agents?.get(agentId)?.provider,
  );
  const cwd = useSessionStore((state) => state.sessions[serverId]?.agents?.get(agentId)?.cwd);
  const sessionId = useSessionStore(
    (state) => state.sessions[serverId]?.agents?.get(agentId)?.runtimeInfo?.sessionId,
  );
  const queryClient = useQueryClient();
  const queryKey = [
    ...agentCommandsQueryKey({ serverId, agentId, draftConfig }),
    "connection",
    connectionId(client),
    "context",
    provider ?? null,
    cwd ?? null,
    sessionId ?? null,
  ];
  const staleTime = draftConfig ? DRAFT_COMMANDS_STALE_TIME : SESSION_COMMANDS_STALE_TIME;
  const readCatalog = useCallback(() => {
    const state = queryClient.getQueryState<AgentCommandCatalog>(queryKey);
    return state?.status === "success" &&
      state.fetchStatus === "idle" &&
      !state.isInvalidated &&
      Date.now() - state.dataUpdatedAt < staleTime
      ? state.data
      : undefined;
  }, [queryClient, queryKey, staleTime]);

  const query = useQuery({
    queryKey,
    queryFn: async () => {
      if (!client) {
        throw new Error(t("common.errors.daemonClientUnavailable"));
      }
      return fetchAgentCommands({ client, agentId, draftConfig });
    },
    enabled: queryEnabled && !!client && isConnected && (!!agentId || !!draftConfig),
    staleTime,
    retry: 3,
    retryDelay: (attemptIndex) => Math.min(1000 * 2 ** attemptIndex, 5000),
  });

  return {
    commands: query.data?.commands ?? [],
    catalog: query.isFetching || query.isError ? undefined : readCatalog(),
    readCatalog,
    isLoading: query.isPending,
    isError: query.isError,
    error: query.error,
  };
}

import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { selectAgentTimelineState, useSessionStore } from "@/stores/session-store";
import {
  createInitDeferred,
  getInitDeferred,
  getInitKey,
  INIT_TIMEOUT_MS,
  rejectInitDeferred,
  refreshInitTimeout,
} from "@/utils/agent-initialization";
import type { HostRuntimeStore } from "@/runtime/host-runtime";
import { planTimelineResumeFetch, planTimelineTailFetch } from "@/timeline/timeline-sync-plan";
import { i18n } from "@/i18n/i18next";

export type SetAgentInitializing = (agentId: string, initializing: boolean) => void;

export function createHistorySyncTimeoutError(): Error {
  return new Error(`History sync timed out after ${Math.round(INIT_TIMEOUT_MS / 1000)}s`);
}

export function refreshAgentInitializationTimeout(input: {
  key: string;
  agentId: string;
  setAgentInitializing: SetAgentInitializing;
}): void {
  refreshInitTimeout({
    key: input.key,
    onTimeout: () => {
      input.setAgentInitializing(input.agentId, false);
      rejectInitDeferred(input.key, createHistorySyncTimeoutError());
    },
  });
}

export interface EnsureAgentIsInitializedInput {
  serverId: string;
  agentId: string;
  client: Pick<DaemonClient, "fetchAgentTimeline"> | null;
  runtime: Pick<HostRuntimeStore, "fetchAgentTimeline">;
  setAgentInitializing: SetAgentInitializing;
  hostDisconnectedMessage?: string;
}

export function ensureAgentIsInitialized(input: EnsureAgentIsInitializedInput): Promise<void> {
  const { serverId, agentId, client, setAgentInitializing } = input;
  const key = getInitKey(serverId, agentId);
  const existing = getInitDeferred(key);
  if (existing) {
    return existing.promise;
  }

  const timeline = selectAgentTimelineState(useSessionStore.getState().sessions[serverId], agentId);
  const timelineRequest = planTimelineResumeFetch(
    timeline.status === "synced" ? timeline.range : null,
  );

  const deferred = createInitDeferred(key, timelineRequest.direction);
  refreshAgentInitializationTimeout({ key, agentId, setAgentInitializing });

  setAgentInitializing(agentId, true);

  if (!client) {
    setAgentInitializing(agentId, false);
    rejectInitDeferred(
      key,
      new Error(input.hostDisconnectedMessage ?? i18n.t("workspace.terminal.hostDisconnected")),
    );
    return deferred.promise;
  }

  input.runtime.fetchAgentTimeline(serverId, agentId, timelineRequest).catch((error) => {
    setAgentInitializing(agentId, false);
    rejectInitDeferred(key, error instanceof Error ? error : new Error(String(error)));
  });

  return deferred.promise;
}

export interface RefreshAgentInput {
  serverId: string;
  agentId: string;
  client: Pick<DaemonClient, "refreshAgent"> | null;
  runtime: Pick<HostRuntimeStore, "fetchAgentTimeline">;
  setAgentInitializing: SetAgentInitializing;
  hostDisconnectedMessage?: string;
}

export async function refreshAgent(input: RefreshAgentInput): Promise<void> {
  const { serverId, agentId, client, runtime, setAgentInitializing } = input;
  if (!client) {
    throw new Error(input.hostDisconnectedMessage ?? i18n.t("workspace.terminal.hostDisconnected"));
  }
  setAgentInitializing(agentId, true);

  try {
    await client.refreshAgent(agentId);
    await runtime.fetchAgentTimeline(serverId, agentId, planTimelineTailFetch());
  } catch (error) {
    setAgentInitializing(agentId, false);
    throw error;
  }
}

export function createSetAgentInitializing(
  serverId: string,
  setInitializingAgents: ReturnType<typeof useSessionStore.getState>["setInitializingAgents"],
): SetAgentInitializing {
  return (agentId, initializing) => {
    setInitializingAgents(serverId, (prev) => {
      if (prev.get(agentId) === initializing) {
        return prev;
      }
      const next = new Map(prev);
      next.set(agentId, initializing);
      return next;
    });
  };
}

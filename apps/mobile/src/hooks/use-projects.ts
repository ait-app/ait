import { useCallback, useEffect, useMemo, useRef, useSyncExternalStore } from "react";
import equal from "fast-deep-equal";
import { useStoreWithEqualityFn } from "zustand/traditional";
import {
  getHostRuntimeStore,
  isHostRuntimeDirectoryLoading,
  useHosts,
  type HostRuntimeSnapshot,
} from "@/runtime/host-runtime";
import {
  useSessionStore,
  type ProjectDescriptor,
  type WorkspaceDescriptor,
} from "@/stores/session-store";
import { buildProjects, type ProjectHost, type ProjectSummary } from "@/utils/projects";

export interface ProjectHostError {
  serverId: string;
  serverName: string;
  message: string;
}

export interface ProjectHostReplica {
  serverId: string;
  serverName: string;
  workspaces: WorkspaceDescriptor[];
  projects: ProjectDescriptor[];
}

export interface ProjectHostRuntimeState {
  serverId: string;
  isOnline: boolean;
  isLoading: boolean;
  error: string | null;
}

interface DerivedProjectsResult {
  projects: ProjectSummary[];
  hostErrors: ProjectHostError[];
  isLoading: boolean;
}

export type UseProjectsResult = DerivedProjectsResult;

interface UseProjectsOptions {
  enabled?: boolean;
}

const EMPTY_PROJECT_HOST_REPLICAS: ProjectHostReplica[] = [];
const EMPTY_PROJECT_HOST_RUNTIME_STATES: ProjectHostRuntimeState[] = [];

function toProjectHostRuntimeState(
  serverId: string,
  snapshot: HostRuntimeSnapshot | null,
): ProjectHostRuntimeState {
  return {
    serverId,
    isOnline: snapshot?.connectionStatus === "online",
    isLoading: isHostRuntimeDirectoryLoading(snapshot),
    error: snapshot?.agentDirectoryError ?? null,
  };
}

function selectProjectHostReplicas(
  hosts: readonly { serverId: string; label: string }[],
  enabled: boolean,
): (state: ReturnType<typeof useSessionStore.getState>) => ProjectHostReplica[] {
  if (!enabled) {
    return () => EMPTY_PROJECT_HOST_REPLICAS;
  }
  return (state) =>
    hosts.map((host) => {
      const session = state.sessions[host.serverId];
      return {
        serverId: host.serverId,
        serverName: host.label,
        workspaces: Array.from(session?.workspaces.values() ?? []),
        projects: Array.from(session?.projects.values() ?? []),
      };
    });
}

export function deriveProjectsFromReplica(input: {
  replicas: readonly ProjectHostReplica[];
  runtimeStates: readonly ProjectHostRuntimeState[];
}): DerivedProjectsResult {
  const runtimeByServerId = new Map(
    input.runtimeStates.map((state) => [state.serverId, state] as const),
  );
  const hosts: ProjectHost[] = input.replicas.map((replica) => {
    const runtimeState = runtimeByServerId.get(replica.serverId);
    return {
      serverId: replica.serverId,
      serverName: replica.serverName,
      isOnline: runtimeState?.isOnline ?? false,
      workspaces: replica.workspaces,
      projects: replica.projects,
    };
  });
  const hostErrors = input.replicas.flatMap((replica) => {
    const message = runtimeByServerId.get(replica.serverId)?.error;
    return message
      ? [
          {
            serverId: replica.serverId,
            serverName: replica.serverName,
            message,
          },
        ]
      : [];
  });

  return {
    ...buildProjects({ hosts }),
    hostErrors,
    isLoading: input.runtimeStates.some((state) => state.isLoading),
  };
}

function useProjectHostRuntimeStates(
  serverIds: readonly string[],
  enabled: boolean,
): ProjectHostRuntimeState[] {
  const runtime = getHostRuntimeStore();
  const previousStatesRef = useRef<ProjectHostRuntimeState[]>([]);
  const subscribe = useCallback(
    (onStoreChange: () => void) =>
      enabled ? runtime.subscribeAll(onStoreChange) : () => undefined,
    [enabled, runtime],
  );
  const getSnapshot = useCallback(() => (enabled ? runtime.getVersion() : 0), [enabled, runtime]);
  const runtimeSnapshotTick = useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
  return useMemo(() => {
    if (!enabled) {
      previousStatesRef.current = EMPTY_PROJECT_HOST_RUNTIME_STATES;
      return EMPTY_PROJECT_HOST_RUNTIME_STATES;
    }
    void runtimeSnapshotTick;
    const nextStates = serverIds.map((serverId) =>
      toProjectHostRuntimeState(serverId, runtime.getSnapshot(serverId)),
    );
    if (equal(previousStatesRef.current, nextStates)) {
      return previousStatesRef.current;
    }
    previousStatesRef.current = nextStates;
    return nextStates;
  }, [enabled, runtime, runtimeSnapshotTick, serverIds]);
}

export function useProjects(options: UseProjectsOptions = {}): UseProjectsResult {
  const enabled = options.enabled ?? true;
  const hosts = useHosts();
  const runtime = getHostRuntimeStore();
  const serverIds = useMemo(
    () => (enabled ? hosts.map((host) => host.serverId) : []),
    [enabled, hosts],
  );
  useEffect(() => {
    const releases = serverIds.map((serverId) => runtime.acquireDirectoryDemand(serverId));
    return () => releases.forEach((release) => release());
  }, [runtime, serverIds]);
  const replicaSelector = useMemo(
    () => selectProjectHostReplicas(hosts, enabled),
    [enabled, hosts],
  );
  const replicas = useStoreWithEqualityFn(useSessionStore, replicaSelector, equal);
  const runtimeStates = useProjectHostRuntimeStates(serverIds, enabled);
  return useMemo(
    () => deriveProjectsFromReplica({ replicas, runtimeStates }),
    [replicas, runtimeStates],
  );
}

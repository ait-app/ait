import { useCallback, useMemo } from "react";
import { useShallow } from "zustand/shallow";
import { useAggregatedAgents } from "@/hooks/use-aggregated-agents";
import {
  getHostRuntimeStore,
  useHosts,
  useHostRuntimeConnectionStatuses,
} from "@/runtime/host-runtime";
import { useSessionStore } from "@/stores/session-store";
import { buildTaskBoard, type TaskBoardHost } from "./model";

/** Directory demand is shared with the sidebar; no per-agent history subscriptions. */
export function useTaskBoard() {
  const hosts = useHosts();
  const directory = useAggregatedAgents();
  const hostIds = useMemo(() => hosts.map((host) => host.serverId), [hosts]);
  const statuses = useHostRuntimeConnectionStatuses(hostIds);
  const agentMaps = useSessionStore(
    useShallow((state) => hostIds.map((id) => state.sessions[id]?.agents)),
  );
  const workspaceMaps = useSessionStore(
    useShallow((state) => hostIds.map((id) => state.sessions[id]?.workspaces)),
  );
  const model = useMemo(() => {
    const inputs: TaskBoardHost[] = hosts.map((host, index) => ({
      serverId: host.serverId,
      label: host.label,
      agents: agentMaps[index] ?? new Map(),
      workspaces: workspaceMaps[index] ?? new Map(),
    }));
    return buildTaskBoard(inputs);
  }, [agentMaps, workspaceMaps, hosts]);
  // The directory hook subscribes to runtime changes, including errors while a
  // connection remains online. Do not present failed loads as an empty board.
  const unavailableHosts = hosts.filter((host) => {
    const snapshot = getHostRuntimeStore().getSnapshot(host.serverId);
    return (
      statuses.get(host.serverId) !== "online" ||
      Boolean(snapshot?.agentDirectoryError || snapshot?.workspaceDirectoryError)
    );
  });
  const allUnavailable = hosts.length > 0 && unavailableHosts.length === hosts.length;
  const connecting = hosts.some((host) => statuses.get(host.serverId) === "connecting");
  const refresh = useCallback(() => {
    const runtime = getHostRuntimeStore();
    for (const host of hosts) {
      if (runtime.getSnapshot(host.serverId)?.connectionStatus !== "online")
        runtime.retryConnection(host.serverId);
    }
    directory.refreshAll();
  }, [hosts, directory.refreshAll]);
  return {
    ...model,
    hosts,
    statuses,
    unavailableHosts,
    allUnavailable,
    isInitialLoad: directory.isInitialLoad && (!allUnavailable || connecting),
    refresh,
  };
}

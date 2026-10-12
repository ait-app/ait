import { useEffect, useMemo } from "react";
import equal from "fast-deep-equal";
import { useStoreWithEqualityFn } from "zustand/traditional";
import { getHostRuntimeStore, useHosts } from "@/runtime/host-runtime";
import { useSessionStore } from "@/stores/session-store";
import type { ScheduleFormWorkspace } from "./schedule-form-model";

export interface ScheduleWorkspaceOption extends ScheduleFormWorkspace {
  serverId: string;
  serverName: string;
}

/** Request the active Workspace directory and keep schedule placement independent of project roots. */
export function useScheduleWorkspaces(): ScheduleWorkspaceOption[] {
  const hosts = useHosts();
  const runtime = getHostRuntimeStore();
  const serverIds = useMemo(() => hosts.map((host) => host.serverId), [hosts]);
  useEffect(() => {
    const releases = serverIds.map((id) => runtime.acquireDirectoryDemand(id));
    return () => releases.forEach((release) => release());
  }, [runtime, serverIds]);
  return useStoreWithEqualityFn(
    useSessionStore,
    useMemo(
      () => (state: ReturnType<typeof useSessionStore.getState>) => {
        return hosts.flatMap((host) =>
          [...(state.sessions[host.serverId]?.workspaces.values() ?? [])]
            .filter((workspace) => !workspace.archivingAt && workspace.workspaceDirectory)
            .map((workspace) => ({
              id: workspace.id,
              serverId: host.serverId,
              serverName: host.label,
              name: workspace.title?.trim() || workspace.name,
              cwd: workspace.workspaceDirectory,
            })),
        );
      },
      [hosts],
    ),
    equal,
  );
}

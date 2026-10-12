import { useQuery } from "@tanstack/react-query";
import { useSessionStore } from "@/stores/session-store";
import { i18n } from "@/i18n/i18next";

export function useWorkspaceSchedules(serverId: string, workspaceId: string, enabled = true) {
  const client = useSessionStore((state) => state.sessions[serverId]?.client ?? null);
  return useQuery({
    queryKey: ["workspace-schedules", serverId, workspaceId],
    queryFn: async () => {
      if (!client) throw new Error(i18n.t("common.errors.daemonClientUnavailable"));
      const result = await client.scheduleList({ workspaceId });
      if (result.error) throw new Error(result.error);
      return result.schedules;
    },
    enabled: enabled && Boolean(client),
    refetchInterval: enabled ? 5_000 : false,
    staleTime: 2_000,
  });
}

export function useScheduleDetail(
  serverId: string,
  scheduleId: string | undefined,
  enabled = true,
) {
  const client = useSessionStore((state) => state.sessions[serverId]?.client ?? null);
  return useQuery({
    queryKey: ["schedule-detail", serverId, scheduleId],
    queryFn: async () => {
      if (!client || !scheduleId) throw new Error(i18n.t("common.errors.daemonClientUnavailable"));
      const result = await client.scheduleInspect({ id: scheduleId });
      if (result.error) throw new Error(result.error);
      if (!result.schedule) throw new Error(i18n.t("schedules.loadDetailError"));
      return result.schedule;
    },
    enabled: enabled && Boolean(client && scheduleId),
    refetchInterval: enabled
      ? (query) => (query.state.data?.runs.some((run) => run.status === "running") ? 1_000 : 5_000)
      : false,
    staleTime: 2_000,
  });
}

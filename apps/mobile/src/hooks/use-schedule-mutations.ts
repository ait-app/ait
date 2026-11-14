import { useCallback } from "react";
import {
  useMutation,
  useQueryClient,
  type QueryClient,
  type QueryKey,
} from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type {
  CreateScheduleOptions,
  DaemonClient,
  UpdateScheduleOptions,
} from "@ait/client/internal/daemon-client";
import type { ScheduleSummary } from "@ait/protocol/schedule/types";
import type {
  AggregatedSchedule,
  FetchAggregatedSchedulesState,
} from "@/schedules/aggregated-schedules";
import { schedulesQueryBaseKey } from "@/schedules/aggregated-schedules";
import { useSessionStore } from "@/stores/session-store";

export type CreateScheduleInput = Omit<CreateScheduleOptions, "requestId">;
export type UpdateScheduleInput = Omit<UpdateScheduleOptions, "requestId">;

interface UseScheduleMutationsResult {
  createSchedule: (input: CreateScheduleInput) => Promise<ScheduleSummary>;
  updateSchedule: (input: UpdateScheduleInput) => Promise<void>;
  pauseSchedule: (id: string) => Promise<void>;
  resumeSchedule: (id: string) => Promise<void>;
  deleteSchedule: (id: string) => Promise<void>;
  runScheduleNow: (id: string) => Promise<void>;
  isCreating: boolean;
  isUpdating: boolean;
  isRunningNow: boolean;
}

interface ScheduleListSnapshot {
  previous: [QueryKey, FetchAggregatedSchedulesState | undefined][];
}

export function updateAggregatedSchedulesData(
  current: FetchAggregatedSchedulesState | undefined,
  updateSchedules: (schedules: AggregatedSchedule[]) => AggregatedSchedule[],
): FetchAggregatedSchedulesState | undefined {
  if (!current || current.status !== "loaded") {
    return current;
  }
  return { ...current, data: updateSchedules(current.data) };
}

function requireClient(serverId: string, unavailableMessage: string): DaemonClient {
  const client = useSessionStore.getState().sessions[serverId]?.client ?? null;
  if (!client) {
    throw new Error(unavailableMessage);
  }
  return client;
}

function snapshotSchedules(queryClient: QueryClient): ScheduleListSnapshot {
  return {
    previous: queryClient.getQueriesData<FetchAggregatedSchedulesState>({
      queryKey: schedulesQueryBaseKey,
    }),
  };
}

function restoreSchedules(queryClient: QueryClient, snapshot: ScheduleListSnapshot): void {
  for (const [queryKey, previous] of snapshot.previous) {
    queryClient.setQueryData(queryKey, previous);
  }
}

function updateSchedulesData(
  queryClient: QueryClient,
  updateSchedules: (schedules: AggregatedSchedule[]) => AggregatedSchedule[],
): void {
  queryClient.setQueriesData<FetchAggregatedSchedulesState>(
    { queryKey: schedulesQueryBaseKey },
    (current) => updateAggregatedSchedulesData(current, updateSchedules),
  );
}

function setScheduleStatus(
  serverId: string,
  id: string,
  status: ScheduleSummary["status"],
): (schedules: AggregatedSchedule[]) => AggregatedSchedule[] {
  const pausedAt = status === "paused" ? new Date().toISOString() : null;
  return (schedules) =>
    schedules.map((schedule) =>
      schedule.serverId === serverId && schedule.id === id
        ? { ...schedule, status, pausedAt }
        : schedule,
    );
}

function removeSchedule(
  serverId: string,
  id: string,
): (schedules: AggregatedSchedule[]) => AggregatedSchedule[] {
  return (schedules) =>
    schedules.filter((schedule) => !(schedule.serverId === serverId && schedule.id === id));
}

function throwIfError(payload: { error?: string | null }): void {
  if (payload.error) {
    throw new Error(payload.error);
  }
}

export function useScheduleMutations({
  serverId,
}: {
  serverId: string;
}): UseScheduleMutationsResult {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  const invalidate = useCallback(() => {
    void queryClient.invalidateQueries({ queryKey: schedulesQueryBaseKey });
    void queryClient.invalidateQueries({ queryKey: ["workspace-schedules"] });
    void queryClient.invalidateQueries({ queryKey: ["schedule-detail"] });
  }, [queryClient]);

  const getClient = () => requireClient(serverId, t("common.errors.daemonClientUnavailable"));

  const applyOptimisticUpdate = async (
    updateSchedules: (schedules: AggregatedSchedule[]) => AggregatedSchedule[],
  ): Promise<ScheduleListSnapshot> => {
    await queryClient.cancelQueries({ queryKey: schedulesQueryBaseKey });
    const snapshot = snapshotSchedules(queryClient);
    updateSchedulesData(queryClient, updateSchedules);
    return snapshot;
  };

  const restoreOnError = (
    _error: unknown,
    _id: string,
    context: ScheduleListSnapshot | undefined,
  ): void => {
    if (context) {
      restoreSchedules(queryClient, context);
    }
  };

  const createMutation = useMutation({
    mutationFn: async (input: CreateScheduleInput): Promise<ScheduleSummary> => {
      const payload = await getClient().scheduleCreate(input);
      throwIfError(payload);
      if (!payload.schedule) throw new Error(t("schedules.loadError"));
      return payload.schedule;
    },
    onSettled: invalidate,
  });

  const updateMutation = useMutation({
    mutationFn: async (input: UpdateScheduleInput): Promise<void> => {
      throwIfError(await getClient().scheduleUpdate(input));
    },
    onSettled: invalidate,
  });

  const pauseMutation = useMutation({
    mutationFn: async (id: string): Promise<void> => {
      throwIfError(await getClient().schedulePause({ id }));
    },
    onMutate: (id) => applyOptimisticUpdate(setScheduleStatus(serverId, id, "paused")),
    onError: restoreOnError,
    onSettled: invalidate,
  });

  const resumeMutation = useMutation({
    mutationFn: async (id: string): Promise<void> => {
      throwIfError(await getClient().scheduleResume({ id }));
    },
    onMutate: (id) => applyOptimisticUpdate(setScheduleStatus(serverId, id, "active")),
    onError: restoreOnError,
    onSettled: invalidate,
  });

  const deleteMutation = useMutation({
    mutationFn: async (id: string): Promise<void> => {
      throwIfError(await getClient().scheduleDelete({ id }));
    },
    onMutate: (id) => applyOptimisticUpdate(removeSchedule(serverId, id)),
    onError: restoreOnError,
    onSettled: invalidate,
  });

  const runNowMutation = useMutation({
    mutationFn: async (id: string): Promise<void> => {
      throwIfError(await getClient().scheduleRunOnce({ id }));
    },
    onSettled: invalidate,
  });

  return {
    createSchedule: createMutation.mutateAsync,
    updateSchedule: updateMutation.mutateAsync,
    pauseSchedule: pauseMutation.mutateAsync,
    resumeSchedule: resumeMutation.mutateAsync,
    deleteSchedule: deleteMutation.mutateAsync,
    runScheduleNow: runNowMutation.mutateAsync,
    isCreating: createMutation.isPending,
    isUpdating: updateMutation.isPending,
    isRunningNow: runNowMutation.isPending,
  };
}

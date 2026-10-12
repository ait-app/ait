import { useState } from "react";
import { Pressable, ScrollView, Text, View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import type { ScheduleRun, StoredSchedule } from "@ait/protocol/schedule/types";
import { Button } from "@/components/ui/button";
import { StatusBadge } from "@/components/ui/status-badge";
import { LoadingSpinner } from "@/components/ui/loading-spinner";
import { WorkspaceScheduleForm } from "./schedule-form-sheet";
import { useAggregatedAgents } from "@/hooks/use-aggregated-agents";
import { useScheduleMutations } from "@/hooks/use-schedule-mutations";
import { useScheduleDetail, useWorkspaceSchedules } from "@/schedules/use-workspace-schedules";
import type { ScheduleFormWorkspace } from "@/schedules/schedule-form-model";
import { formatCadence, resolveScheduleTitle } from "@/utils/schedule-format";
import { confirmDialog } from "@/utils/confirm-dialog";
import { toErrorMessage } from "@/utils/error-messages";

interface WorkspaceScheduleTabProps {
  serverId: string;
  workspace: ScheduleFormWorkspace;
  scheduleId?: string;
  view?: "list";
  onBrowse: () => void;
  interactive: boolean;
  onOpen: (scheduleId: string) => void;
  onClose: () => void;
  onCreate: () => void;
  onOpenAgent: (agentId: string) => void;
}

export function WorkspaceScheduleTab(props: WorkspaceScheduleTabProps) {
  const { t } = useTranslation();
  const view = props.view === "list" ? "list" : "form";
  const [editing, setEditing] = useState(false);
  const detail = useScheduleDetail(props.serverId, props.scheduleId, props.interactive);
  const list = useWorkspaceSchedules(
    props.serverId,
    props.workspace.id,
    props.interactive && view === "list",
  );
  let content;
  if (view === "list") {
    content = (
      <ScrollView contentContainerStyle={styles.content} testID="workspace-schedules-list">
        <Text style={styles.title}>{props.workspace.name}</Text>
        {list.isPending ? (
          <LoadingSpinner color={styles.muted.color} />
        ) : list.isError ? (
          <QueryError
            message={t("schedules.loadError")}
            onRetry={() => {
              void list.refetch();
            }}
          />
        ) : list.data?.length ? (
          list.data.map((schedule) => (
            <Pressable
              key={schedule.id}
              style={styles.card}
              accessibilityRole="button"
              accessibilityLabel={resolveScheduleTitle(schedule, t)}
              testID={`workspace-schedule-${schedule.id}`}
              onPress={() => props.onOpen(schedule.id)}
            >
              <View style={styles.row}>
                <Text style={styles.cardTitle}>{resolveScheduleTitle(schedule, t)}</Text>
                <StatusBadge
                  label={t(
                    schedule.status === "completed"
                      ? "schedules.status.finished"
                      : `schedules.status.${schedule.status}`,
                  )}
                  variant={schedule.status === "active" ? "success" : "muted"}
                />
              </View>
              <Text style={styles.muted}>{formatCadence(schedule.cadence, t)}</Text>
              <Text style={styles.body} numberOfLines={2}>
                {schedule.prompt}
              </Text>
            </Pressable>
          ))
        ) : (
          <Text style={styles.muted}>{t("schedules.empty")}</Text>
        )}
      </ScrollView>
    );
  } else if (!props.scheduleId) {
    content = (
      <WorkspaceScheduleForm
        serverId={props.serverId}
        workspace={props.workspace}
        onClose={props.onClose}
        onSaved={(schedule) => props.onOpen(schedule.id)}
      />
    );
  } else if (detail.isPending) {
    content = (
      <View style={styles.centered}>
        <LoadingSpinner color={styles.muted.color} />
      </View>
    );
  } else if (detail.isError || !detail.data) {
    content = (
      <QueryError
        message={t("schedules.loadDetailError")}
        onRetry={() => {
          void detail.refetch();
        }}
      />
    );
  } else if (detail.data.workspaceId !== props.workspace.id) {
    content = <QueryError message={t("schedules.taskMissing")} />;
  } else if (editing) {
    content = (
      <WorkspaceScheduleForm
        key={`edit:${props.scheduleId}`}
        serverId={props.serverId}
        workspace={props.workspace}
        schedule={detail.data}
        onClose={() => setEditing(false)}
        onSaved={() => setEditing(false)}
      />
    );
  } else {
    content = (
      <ScheduleDetails
        {...props}
        schedule={detail.data}
        onEdit={() => setEditing(true)}
        onDeleted={props.onBrowse}
        refresh={() => {
          void detail.refetch();
        }}
      />
    );
  }
  return (
    <View style={styles.container}>
      <View style={styles.toolbar}>
        <Button
          size="sm"
          variant={view === "list" ? "secondary" : "ghost"}
          onPress={props.onBrowse}
          testID="workspace-schedules-browse"
        >
          {t("schedules.browse")}
        </Button>
        <Button
          size="sm"
          variant="outline"
          onPress={props.onCreate}
          testID="workspace-schedules-new"
        >
          {t("schedules.new")}
        </Button>
      </View>
      {content}
    </View>
  );
}

function ScheduleDetails({
  schedule,
  serverId,
  workspace,
  onEdit,
  onDeleted,
  onOpenAgent,
  refresh,
}: WorkspaceScheduleTabProps & {
  schedule: StoredSchedule;
  onEdit: () => void;
  onDeleted: () => void;
  refresh: () => void;
}) {
  const { t, i18n } = useTranslation();
  const mutations = useScheduleMutations({ serverId });
  const queryClient = useQueryClient();
  const [pending, setPending] = useState<ReadonlySet<string>>(() => new Set());
  const [error, setError] = useState<string | null>(null);
  const [expandedRun, setExpandedRun] = useState<string | null>(null);
  const running = mutations.isRunningNow || schedule.runs.some((run) => run.status === "running");
  const { agents } = useAggregatedAgents({ includeArchived: true });
  const targetAgentId = schedule.target.type === "agent" ? schedule.target.agentId : null;
  const targetAgent = agents.find(
    (agent) => agent.serverId === serverId && agent.id === targetAgentId,
  );
  const ended = schedule.status === "completed";
  const title = resolveScheduleTitle(schedule, t);
  const date = (iso: string) => new Date(iso).toLocaleString(i18n.resolvedLanguage);
  const runAction = async (action: string, execute: () => Promise<void>) => {
    setPending((current) => new Set([...current, action]));
    setError(null);
    void queryClient.invalidateQueries({ queryKey: ["schedule-detail", serverId, schedule.id] });
    try {
      await execute();
      refresh();
    } catch (error) {
      setError(toErrorMessage(error));
    } finally {
      setPending((current) => {
        const next = new Set(current);
        next.delete(action);
        return next;
      });
    }
  };
  const deleteTask = async () => {
    const confirmed = await confirmDialog({
      title: t("schedules.delete", { product: t("schedules.product.schedule") }),
      message: t("schedules.deleteConfirm", { title }),
      confirmLabel: t("schedules.deleteAction"),
      cancelLabel: t("common.actions.cancel"),
      destructive: true,
    });
    if (confirmed)
      await runAction("delete", async () => {
        await mutations.deleteSchedule(schedule.id);
        onDeleted();
      });
  };
  return (
    <ScrollView contentContainerStyle={styles.content} testID="workspace-schedule-details">
      <View style={styles.row}>
        <Text style={styles.title}>{title}</Text>
        <StatusBadge
          label={
            running
              ? t("schedules.running")
              : t(ended ? "schedules.status.finished" : `schedules.status.${schedule.status}`)
          }
          variant={
            running ? "warning" : ended || schedule.status === "paused" ? "muted" : "success"
          }
        />
      </View>
      <Text style={styles.muted}>
        {workspace.name} · {formatCadence(schedule.cadence, t)}
      </Text>
      <Text style={styles.body}>
        {t("schedules.form.target")}:{" "}
        {schedule.target.type === "new-agent"
          ? t("schedules.form.newAgent")
          : targetAgent?.title?.trim() || t("schedules.agentUnavailable")}
      </Text>
      {schedule.target.type === "agent" && targetAgent?.workspaceId === workspace.id ? (
        <Button variant="ghost" size="sm" onPress={() => onOpenAgent(targetAgent.id)}>
          {t("schedules.openAgent")}
        </Button>
      ) : null}
      <View style={styles.actions}>
        {!ended ? (
          <>
            <Button
              size="sm"
              onPress={() => {
                void runAction("run", () => mutations.runScheduleNow(schedule.id));
              }}
              loading={pending.has("run")}
              disabled={pending.size > 0 || running}
              testID="schedule-run-now"
            >
              {t("schedules.runNow")}
            </Button>
            <Button
              size="sm"
              variant="outline"
              onPress={() => {
                void runAction("status", () =>
                  schedule.status === "paused"
                    ? mutations.resumeSchedule(schedule.id)
                    : mutations.pauseSchedule(schedule.id),
                );
              }}
              disabled={pending.has("status") || pending.has("delete")}
              loading={pending.has("status")}
              testID="schedule-toggle-status"
            >
              {t(schedule.status === "paused" ? "schedules.resume" : "schedules.pause")}
            </Button>
            <Button
              size="sm"
              variant="outline"
              onPress={onEdit}
              disabled={pending.size > 0 || running}
              testID="schedule-edit"
            >
              {t("schedules.edit", { product: t("schedules.product.schedule") })}
            </Button>
          </>
        ) : null}
        <Button
          size="sm"
          variant="ghost"
          onPress={() => {
            void deleteTask();
          }}
          disabled={pending.size > 0 || running}
          loading={pending.has("delete")}
          testID="schedule-delete"
        >
          {t("schedules.deleteAction")}
        </Button>
      </View>
      {error ? (
        <Text style={styles.error} accessibilityRole="alert">
          {error}
        </Text>
      ) : null}
      <View style={styles.card}>
        <Text style={styles.cardTitle}>{t("schedules.prompt")}</Text>
        <Text style={styles.body} selectable>
          {schedule.prompt}
        </Text>
        <Text style={styles.muted}>
          {t("schedules.runCount", {
            count: schedule.runs.filter((run) => run.status !== "running").length,
            limit: schedule.maxRuns ?? t("schedules.unlimited"),
          })}
        </Text>
        {schedule.nextRunAt ? (
          <Text style={styles.muted}>
            {t("schedules.nextAt", { time: date(schedule.nextRunAt) })}
          </Text>
        ) : null}
        {schedule.expiresAt ? (
          <Text style={styles.muted}>
            {t("schedules.form.expiresAt")}: {date(schedule.expiresAt)}
          </Text>
        ) : null}
      </View>
      <Text style={styles.cardTitle}>{t("schedules.history")}</Text>
      {schedule.runs.length ? (
        [...schedule.runs]
          .reverse()
          .map((run) => (
            <ScheduleRunCard
              key={run.id}
              run={run}
              expanded={expandedRun === run.id}
              onToggle={() => setExpandedRun(expandedRun === run.id ? null : run.id)}
              onOpenAgent={run.workspaceId === workspace.id ? onOpenAgent : undefined}
            />
          ))
      ) : (
        <Text style={styles.muted}>{t("schedules.noRuns")}</Text>
      )}
    </ScrollView>
  );
}

function ScheduleRunCard({
  run,
  expanded,
  onToggle,
  onOpenAgent,
}: {
  run: ScheduleRun;
  expanded: boolean;
  onToggle: () => void;
  onOpenAgent?: (id: string) => void;
}) {
  const { t, i18n } = useTranslation();
  return (
    <View style={styles.card} testID={`schedule-run-${run.id}`}>
      <Pressable
        onPress={onToggle}
        accessibilityRole="button"
        accessibilityState={{ expanded }}
        accessibilityLabel={`${t(`schedules.${run.status}`)} ${new Date(run.startedAt).toLocaleString(i18n.resolvedLanguage)}`}
      >
        <View style={styles.row}>
          <Text style={styles.body}>
            {new Date(run.startedAt).toLocaleString(i18n.resolvedLanguage)}
          </Text>
          <StatusBadge
            label={t(`schedules.${run.status}`)}
            variant={
              run.status === "failed" ? "error" : run.status === "succeeded" ? "success" : "warning"
            }
          />
        </View>
      </Pressable>
      {run.error ? (
        <Text style={styles.error} selectable>
          {run.error}
        </Text>
      ) : null}
      {run.agentId && onOpenAgent ? (
        <Button
          size="sm"
          variant="ghost"
          onPress={() => onOpenAgent(run.agentId!)}
          testID={`schedule-run-agent-${run.id}`}
        >
          {t("schedules.openAgent")}
        </Button>
      ) : null}
      {expanded && run.output ? (
        <Text style={styles.output} selectable testID={`schedule-run-output-${run.id}`}>
          {run.output}
        </Text>
      ) : null}
    </View>
  );
}

function QueryError({ message, onRetry }: { message: string; onRetry?: () => void }) {
  const { t } = useTranslation();
  return (
    <View style={styles.centered}>
      <Text style={styles.error}>{message}</Text>
      {onRetry ? (
        <Button variant="ghost" onPress={onRetry}>
          {t("schedules.retry")}
        </Button>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  container: { flex: 1, minHeight: 0, backgroundColor: theme.colors.surface0 },
  toolbar: {
    flexDirection: "row",
    flexWrap: "wrap",
    justifyContent: "space-between",
    gap: theme.spacing[2],
    padding: theme.spacing[3],
    borderBottomWidth: 1,
    borderBottomColor: theme.colors.border,
  },
  content: {
    padding: theme.spacing[4],
    gap: theme.spacing[4],
    width: "100%",
    maxWidth: 900,
    alignSelf: "center",
  },
  centered: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
    padding: theme.spacing[4],
    gap: theme.spacing[3],
  },
  row: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    flexWrap: "wrap",
    gap: theme.spacing[2],
  },
  actions: { flexDirection: "row", flexWrap: "wrap", gap: theme.spacing[2] },
  title: {
    fontSize: theme.fontSize.lg,
    fontWeight: theme.fontWeight.semibold,
    color: theme.colors.foreground,
    flexShrink: 1,
  },
  cardTitle: {
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.semibold,
    color: theme.colors.foreground,
    flexShrink: 1,
  },
  body: { color: theme.colors.foreground, fontSize: theme.fontSize.sm },
  muted: { color: theme.colors.foregroundMuted, fontSize: theme.fontSize.sm },
  error: { color: theme.colors.destructive, fontSize: theme.fontSize.sm },
  card: {
    padding: theme.spacing[4],
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: theme.borderRadius.lg,
    gap: theme.spacing[3],
  },
  output: {
    fontFamily: theme.fontFamily.mono,
    fontSize: theme.fontSize.sm,
    color: theme.colors.foreground,
  },
}));

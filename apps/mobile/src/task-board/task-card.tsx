import React, { memo } from "react";
import { Pressable, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import {
  CircleCheck,
  CircleDot,
  CircleAlert,
  Clock3,
  Folder,
  GitFork,
  Monitor,
  WifiOff,
} from "lucide-react-native";
import type { WorkspaceStateBucket } from "@ait/protocol/agent-state-bucket";
import { getProviderIcon } from "@/components/provider-icons";
import { useCompactTimeAgo } from "@/hooks/use-compact-time-ago";
import type { Theme } from "@/styles/theme";
import { taskDescendants, type BoardTask } from "./model";

export function taskStatusColor(theme: Theme, bucket: WorkspaceStateBucket): string {
  if (bucket === "needs_input") return theme.colors.statusWarning;
  if (bucket === "failed") return theme.colors.statusDanger;
  if (bucket === "running") return theme.colors.statusDotRunning;
  return theme.colors.statusSuccess;
}

export function TaskStatus({ bucket }: { bucket: WorkspaceStateBucket }) {
  const { theme } = useUnistyles();
  const { t } = useTranslation();
  const color = taskStatusColor(theme, bucket);
  const Icon =
    bucket === "needs_input"
      ? Clock3
      : bucket === "failed"
        ? CircleAlert
        : bucket === "done"
          ? CircleCheck
          : CircleDot;
  return (
    <View style={styles.line}>
      <Icon size={14} color={color} />
      <Text style={[styles.status, { color }]}>{t(`taskBoard.status.${bucket}`)}</Text>
    </View>
  );
}

export function TaskProvider({ task }: { task: BoardTask }) {
  const { theme } = useUnistyles();
  const ProviderIcon = getProviderIcon(task.agent.provider, task.agent.serverId);
  const model = task.agent.model ?? task.agent.runtimeInfo?.model;
  return (
    <View style={styles.line}>
      <ProviderIcon size={15} color={theme.colors.foregroundMuted} />
      <Text style={styles.meta} numberOfLines={1}>
        {task.agent.provider}
        {model ? ` · ${model}` : ""}
      </Text>
    </View>
  );
}

function TaskTime({ task }: { task: BoardTask }) {
  const { t } = useTranslation();
  const startedAt = task.agent.turn.phase === "open" ? task.agent.turn.startedAt : null;
  const time = useCompactTimeAgo(
    task.bucket === "running" && startedAt ? startedAt : new Date(task.activityAt),
  );
  const duration = /^(\d+)([mhd])$/.exec(time);
  const running = task.bucket === "running" && startedAt;
  if (time === "now") return <Text style={styles.time}>{t("schedules.time.now")}</Text>;
  if (!duration) return <Text style={styles.time}>{time}</Text>;
  const units = { m: "minutesAgo", h: "hoursAgo", d: "daysAgo" } as const;
  return (
    <Text style={styles.time}>
      {running
        ? t("taskBoard.elapsed", { time })
        : t(`schedules.time.${units[duration[2] as keyof typeof units]}`, {
            count: Number(duration[1]),
          })}
    </Text>
  );
}

export const TaskCard = memo(function TaskCard({
  task,
  selected,
  online,
  onSelect,
}: {
  task: BoardTask;
  selected: boolean;
  online: boolean;
  onSelect: (key: string) => void;
}) {
  const { theme } = useUnistyles();
  const { t } = useTranslation();
  const descendants = taskDescendants(task);
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityState={{ selected }}
      accessibilityLabel={`${task.agent.title || t("agentList.fallbackTitle")}, ${t(`taskBoard.status.${task.bucket}`)}`}
      testID={`task-card-${task.agent.serverId}-${task.agent.id}`}
      onPress={() => onSelect(task.key)}
      style={({ pressed, hovered = false }: { pressed: boolean; hovered?: boolean }) => [
        styles.card,
        hovered && styles.hovered,
        selected && styles.selected,
        pressed && styles.pressed,
      ]}
    >
      <View style={styles.line}>
        <Folder size={14} color={theme.colors.foregroundMuted} />
        <Text style={styles.meta} numberOfLines={1}>
          {task.projectName}
          {task.workspaceName !== task.projectName ? ` / ${task.workspaceName}` : ""}
        </Text>
      </View>
      <Text style={styles.title} numberOfLines={2}>
        {task.agent.title || t("agentList.fallbackTitle")}
      </Text>
      <TaskProvider task={task} />
      <View style={styles.line}>
        {online ? (
          <Monitor size={14} color={theme.colors.foregroundMuted} />
        ) : (
          <WifiOff size={14} color={theme.colors.statusWarning} />
        )}
        <Text style={styles.meta} numberOfLines={1}>
          {task.hostLabel}
          {!online ? ` · ${t("taskBoard.offline")}` : ""}
        </Text>
      </View>
      <View style={styles.footer}>
        <TaskStatus bucket={task.bucket} />
        <TaskTime task={task} />
      </View>
      {task.ownBucket === "failed" && task.agent.lastError ? (
        <Text style={styles.error} numberOfLines={2}>
          {task.agent.lastError}
        </Text>
      ) : null}
      {descendants.length ? (
        <View style={styles.children}>
          <GitFork size={14} color={theme.colors.foregroundMuted} />
          <Text style={styles.meta}>
            {t("taskBoard.childrenCount", { count: descendants.length })}
          </Text>
        </View>
      ) : null}
    </Pressable>
  );
});

const styles = StyleSheet.create((theme) => ({
  card: {
    backgroundColor: theme.colors.surface0,
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: theme.borderRadius.lg,
    padding: theme.spacing[4],
    gap: theme.spacing[2],
    marginBottom: theme.spacing[3],
  },
  selected: { borderColor: theme.colors.accent },
  hovered: { backgroundColor: theme.colors.surface1, borderColor: theme.colors.foregroundMuted },
  pressed: { opacity: 0.8 },
  line: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
    minWidth: 0,
    flexShrink: 1,
  },
  title: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.xl,
    fontWeight: theme.fontWeight.semibold,
    lineHeight: 23,
    marginVertical: theme.spacing[1],
  },
  meta: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.base,
    lineHeight: 21,
    flexShrink: 1,
  },
  footer: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    flexWrap: "wrap",
    gap: theme.spacing[2],
    marginTop: theme.spacing[2],
  },
  status: { fontSize: theme.fontSize.base, lineHeight: 21, flexShrink: 1 },
  time: { color: theme.colors.foregroundMuted, fontSize: theme.fontSize.sm },
  error: { color: theme.colors.statusDanger, fontSize: theme.fontSize.sm, lineHeight: 19 },
  children: {
    borderTopWidth: 1,
    borderTopColor: theme.colors.border,
    paddingTop: theme.spacing[2],
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
  },
}));

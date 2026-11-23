import React, { memo, useEffect, useState } from "react";
import { Pressable, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import {
  Archive,
  ArrowUpRight,
  Brain,
  Check,
  ChevronDown,
  ChevronRight,
  Clock3,
  GitBranch,
  GitFork,
  Hourglass,
  Monitor,
  Power,
  WifiOff,
} from "lucide-react-native";
import { getProviderIcon } from "@/components/provider-icons";
import { ProjectIconView } from "@/components/project-icon-view";
import { PermissionRequestCard } from "@/components/permission-request-card";
import { Button } from "@/components/ui/button";
import { useCompactTimeAgo } from "@/hooks/use-compact-time-ago";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { formatDuration } from "@/utils/time";
import { toErrorMessage } from "@/utils/error-messages";
import { taskDescendants, type BoardTask } from "./model";
import { TaskStatusIcon } from "./task-status";
import { useTaskActions } from "./use-task-actions";

export function TaskProvider({ task }: { task: BoardTask }) {
  const { theme } = useUnistyles();
  const ProviderIcon = getProviderIcon(task.agent.provider, task.agent.serverId);
  const model = task.agent.model ?? task.agent.runtimeInfo?.model;
  const effort = task.agent.thinkingOptionId ?? task.agent.runtimeInfo?.thinkingOptionId;
  return (
    <View style={styles.badgeRow}>
      <View style={styles.providerBadge}>
        <ProviderIcon size={12} color={theme.colors.foregroundMuted} />
        <Text style={styles.badgeText} numberOfLines={1}>
          {model || task.agent.provider}
        </Text>
        {effort ? (
          <>
            <Brain size={12} color={theme.colors.foregroundMuted} />
            <Text style={styles.badgeText}>{effort}</Text>
          </>
        ) : null}
      </View>
      {task.branchName ? (
        <View style={styles.branchBadge}>
          <GitBranch size={12} color={theme.colors.statusDotRunning} />
          <Text style={styles.branchText} numberOfLines={1}>
            {task.branchName}
          </Text>
        </View>
      ) : null}
    </View>
  );
}

function TaskTime({ task }: { task: BoardTask }) {
  const { t, i18n } = useTranslation();
  const { theme } = useUnistyles();
  const startedAt =
    task.ownBucket === "running" && task.agent.turn.phase === "open"
      ? task.agent.turn.startedAt
      : null;
  const started = startedAt?.getTime();
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    if (started == null) return;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [started]);
  const ago = useCompactTimeAgo(new Date(task.activityAt));
  const time = ago === "now" ? t("schedules.time.now") : ago;
  return (
    <View style={styles.line}>
      <Clock3 size={12} color={theme.colors.foregroundMuted} />
      <Text style={styles.meta}>
        {startedAt
          ? startedAt.toLocaleTimeString(i18n.language, {
              hour: "2-digit",
              minute: "2-digit",
              hour12: false,
            })
          : time}
      </Text>
      {started != null ? (
        <>
          <Hourglass size={12} color={theme.colors.foregroundMuted} />
          <Text style={styles.meta}>{formatDuration(now - started)}</Text>
        </>
      ) : null}
    </View>
  );
}

export const TaskCard = memo(function TaskCard({
  task,
  selected,
  online,
  showChildren = true,
  iconDataUri = null,
  onSelect,
}: {
  task: BoardTask;
  selected: boolean;
  online: boolean;
  showChildren?: boolean;
  iconDataUri?: string | null;
  onSelect: (key: string) => void;
}) {
  const { theme } = useUnistyles();
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const [hovered, setHovered] = useState(false);
  const descendants = taskDescendants(task);
  const client = useHostRuntimeClient(task.agent.serverId);
  const { action, allowed, disabled, open } = useTaskActions(task, online);
  const observed = task.agent.management === "observed" || Boolean(task.agent.observation);
  const select = () => onSelect(task.key);
  return (
    <View
      testID={`task-card-${task.agent.serverId}-${task.agent.id}`}
      style={[
        styles.card,
        observed && styles.observed,
        task.ownBucket === "failed" && styles.failed,
        hovered && styles.hovered,
        selected && styles.selected,
      ]}
    >
      <Pressable
        accessibilityRole="button"
        accessibilityState={{ selected }}
        accessibilityLabel={`${task.agent.title || t("agentList.fallbackTitle")}, ${t(`taskBoard.status.${task.bucket}`)}`}
        testID="task-summary"
        onPress={select}
        onHoverIn={() => setHovered(true)}
        onHoverOut={() => setHovered(false)}
        style={styles.summary}
      >
        <View style={styles.line}>
          <ProjectIconView
            iconDataUri={iconDataUri}
            initial={task.projectName.slice(0, 1).toUpperCase()}
            projectViewKey={task.projectKey}
            size={12}
            textStyle={styles.projectInitial}
          />
          <Text style={styles.meta} numberOfLines={1}>
            {task.projectName}
            {task.workspaceName !== task.projectName ? ` › ${task.workspaceName}` : ""}
          </Text>
          {observed ? <Text style={styles.readOnly}>{t("taskBoard.readOnly")}</Text> : null}
        </View>
        <View style={styles.titleRow}>
          <View style={styles.titleIcon}>
            <TaskStatusIcon bucket={task.bucket} />
          </View>
          <Text style={styles.title} numberOfLines={2}>
            {task.agent.title || t("agentList.fallbackTitle")}
          </Text>
        </View>
        <TaskProvider task={task} />
        <View style={styles.metadata}>
          <TaskTime task={task} />
          <View style={styles.host}>
            {online ? (
              <Monitor size={12} color={theme.colors.foregroundMuted} />
            ) : (
              <WifiOff size={12} color={theme.colors.statusWarning} />
            )}
            <Text style={[styles.meta, !online && styles.warning]} numberOfLines={1}>
              {task.hostLabel}
              {!online ? ` · ${t("taskBoard.offline")}` : ""}
            </Text>
          </View>
        </View>
      </Pressable>
      {(allowed.approve ? task.agent.pendingPermissions : []).map((request) => (
        <View style={styles.permission} key={request.id}>
          <PermissionRequestCard
            compact
            permission={{ key: request.id, agentId: task.agent.id, request }}
            client={online ? client : null}
            onOpenDetails={select}
          />
        </View>
      ))}
      <View style={styles.actions}>
        {allowed.stop ? (
          <Button
            size="xs"
            variant="outline"
            leftIcon={Power}
            style={styles.stopButton}
            textStyle={styles.danger}
            disabled={disabled}
            loading={action.isPending && action.variables === "stop"}
            testID="task-inline-stop"
            onPress={() => action.mutate("stop")}
          >
            {t("taskBoard.stop")}
          </Button>
        ) : null}
        {allowed.read ? (
          <Button
            size="xs"
            variant="outline"
            leftIcon={Check}
            textStyle={styles.success}
            disabled={disabled}
            loading={action.isPending && action.variables === "read"}
            testID="task-inline-mark-read"
            onPress={() => action.mutate("read")}
          >
            {t("taskBoard.markRead")}
          </Button>
        ) : null}
        {allowed.archive && !allowed.read ? (
          <Button
            size="xs"
            variant="outline"
            leftIcon={Archive}
            accessibilityLabel={t("taskBoard.archive")}
            disabled={disabled}
            loading={action.isPending && action.variables === "archive"}
            testID="task-inline-archive"
            onPress={() => action.mutate("archive")}
          />
        ) : null}
        <Button
          size="xs"
          variant="outline"
          style={styles.openButton}
          trailing={<ArrowUpRight size={12} color={theme.colors.foregroundMuted} />}
          testID="task-inline-open"
          onPress={open}
        >
          {t("taskBoard.open")}
        </Button>
      </View>
      {action.isError ? (
        <Text accessibilityRole="alert" style={styles.error}>
          {toErrorMessage(action.error)}
        </Text>
      ) : null}
      {task.ownBucket === "failed" && task.agent.lastError ? (
        <Text style={styles.error} numberOfLines={3}>
          {task.agent.lastError}
        </Text>
      ) : null}
      {showChildren && descendants.length ? (
        <View style={styles.children}>
          <Pressable
            style={styles.childToggle}
            accessibilityRole="button"
            accessibilityState={{ expanded }}
            onPress={() => setExpanded(!expanded)}
            testID="task-inline-children"
          >
            {expanded ? (
              <ChevronDown size={12} color={theme.colors.foregroundMuted} />
            ) : (
              <ChevronRight size={12} color={theme.colors.foregroundMuted} />
            )}
            <GitFork size={12} color={theme.colors.foregroundMuted} />
            <Text style={styles.meta}>
              {t("taskBoard.childrenCount", { count: descendants.length })}
            </Text>
          </Pressable>
          {expanded
            ? descendants.map((child) => (
                <Pressable
                  key={child.key}
                  style={styles.childRow}
                  accessibilityRole="button"
                  onPress={() => onSelect(child.key)}
                  testID={`task-inline-child-${child.agent.id}`}
                >
                  <TaskStatusIcon bucket={child.ownBucket} />
                  <Text style={styles.childTitle} numberOfLines={2}>
                    {child.agent.title || t("agentList.fallbackTitle")}
                  </Text>
                  <ArrowUpRight size={12} color={theme.colors.foregroundMuted} />
                </Pressable>
              ))
            : null}
        </View>
      ) : null}
    </View>
  );
});

const styles = StyleSheet.create((theme) => ({
  card: {
    backgroundColor: theme.colors.surface0,
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: theme.borderRadius.lg,
    marginBottom: theme.spacing[3],
    overflow: "hidden",
  },
  observed: { borderStyle: "dashed" },
  failed: { borderColor: theme.colors.statusDanger },
  selected: { borderColor: theme.colors.accent },
  hovered: { borderColor: theme.colors.foregroundMuted },
  summary: { padding: theme.spacing[3], gap: theme.spacing[1.5] },
  line: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[1],
    minWidth: 0,
    flexShrink: 1,
  },
  titleRow: { flexDirection: "row", alignItems: "flex-start", gap: theme.spacing[1.5] },
  titleIcon: { paddingTop: theme.spacing[1] },
  title: {
    flex: 1,
    minWidth: 0,
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
    lineHeight: 20,
  },
  projectInitial: { fontSize: 8, lineHeight: 10, fontWeight: theme.fontWeight.semibold },
  meta: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    lineHeight: 16,
    flexShrink: 1,
  },
  readOnly: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    marginLeft: "auto",
  },
  badgeRow: { flexDirection: "row", flexWrap: "wrap", gap: theme.spacing[1], minWidth: 0 },
  providerBadge: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[1],
    backgroundColor: theme.colors.surface2,
    paddingHorizontal: theme.spacing[1.5],
    paddingVertical: theme.spacing[0.5],
    borderRadius: theme.borderRadius.full,
    maxWidth: "100%",
    flexShrink: 1,
  },
  badgeText: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    lineHeight: 16,
    flexShrink: 1,
  },
  branchBadge: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[1],
    backgroundColor: `${theme.colors.statusDotRunning}15`,
    paddingHorizontal: theme.spacing[1.5],
    paddingVertical: theme.spacing[0.5],
    borderRadius: theme.borderRadius.full,
    maxWidth: "100%",
    flexShrink: 1,
  },
  branchText: {
    color: theme.colors.statusDotRunning,
    fontSize: theme.fontSize.sm,
    lineHeight: 16,
    flexShrink: 1,
  },
  metadata: {
    flexDirection: "row",
    flexWrap: "wrap",
    alignItems: "center",
    gap: theme.spacing[2],
    marginTop: theme.spacing[1],
  },
  host: { flexDirection: "row", alignItems: "center", gap: theme.spacing[1], flexShrink: 1 },
  permission: { paddingHorizontal: theme.spacing[3], paddingBottom: theme.spacing[2] },
  actions: {
    flexDirection: "row",
    flexWrap: "wrap",
    alignItems: "center",
    gap: theme.spacing[1.5],
    paddingHorizontal: theme.spacing[3],
    paddingBottom: theme.spacing[3],
  },
  openButton: { flexGrow: 1, flexBasis: 84, minWidth: 84 },
  stopButton: { borderColor: theme.colors.statusDanger },
  danger: { color: theme.colors.statusDanger },
  success: { color: theme.colors.statusSuccess },
  warning: { color: theme.colors.statusWarning },
  error: {
    color: theme.colors.statusDanger,
    fontSize: theme.fontSize.sm,
    lineHeight: 18,
    padding: theme.spacing[3],
    borderTopWidth: 1,
    borderTopColor: theme.colors.border,
  },
  children: {
    borderTopWidth: 1,
    borderTopColor: theme.colors.border,
    padding: theme.spacing[2],
    gap: theme.spacing[1.5],
  },
  childToggle: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[1.5],
    minHeight: 28,
  },
  childRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[1.5],
    paddingVertical: theme.spacing[1.5],
    paddingHorizontal: theme.spacing[1],
  },
  childTitle: {
    flex: 1,
    color: theme.colors.foreground,
    fontSize: theme.fontSize.sm,
    lineHeight: 18,
  },
}));

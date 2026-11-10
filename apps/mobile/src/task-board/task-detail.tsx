import React, { useMemo } from "react";
import { Pressable, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery } from "@tanstack/react-query";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import { Archive, ArrowUpRight, Check, GitFork, Square, X } from "lucide-react-native";
import { Button } from "@/components/ui/button";
import { LoadingSpinner } from "@/components/ui/loading-spinner";
import { ScrollView } from "@/components/ui/scroll-view";
import { PermissionRequestCard } from "@/components/permission-request-card";
import { useToast } from "@/contexts/toast-context";
import { useArchiveAgent } from "@/hooks/use-archive-agent";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { navigateToAgent } from "@/utils/navigate-to-agent";
import { toErrorMessage } from "@/utils/error-messages";
import { taskActions, taskDescendants, type BoardTask } from "./model";
import { TaskProvider, TaskStatus } from "./task-card";
import { useSubagentsForParent } from "@/subagents";

function ProviderSubtasks({ task }: { task: BoardTask }) {
  const { t } = useTranslation();
  const rows = useSubagentsForParent({
    serverId: task.agent.serverId,
    parentAgentId: task.agent.id,
  }).filter((row) => row.kind === "provider");
  if (!rows.length) return null;
  return (
    <View style={styles.section}>
      <Text style={styles.sectionTitle}>
        {t("subagents.title")} · {task.agent.provider}
      </Text>
      {rows.map((row) => (
        <View style={styles.child} key={row.id}>
          <Text style={styles.childTitle}>
            {row.description || row.title || t("agentList.fallbackTitle")}
          </Text>
          {row.subtitle ? <Text style={styles.meta}>{row.subtitle}</Text> : null}
          <TaskStatus
            bucket={
              row.status === "running" ? "running" : row.status === "failed" ? "failed" : "done"
            }
          />
        </View>
      ))}
    </View>
  );
}

export function TaskDetail({
  task,
  parent,
  online,
  showHeader = true,
  onSelect,
  onClose,
}: {
  task: BoardTask;
  parent?: BoardTask;
  showHeader?: boolean;
  online: boolean;
  onSelect: (key: string) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const { theme } = useUnistyles();
  const { agent } = task;
  const client = useHostRuntimeClient(agent.serverId);
  const { archiveAgent } = useArchiveAgent();
  const toast = useToast();
  const children = useMemo(() => taskDescendants(task), [task]);
  const running =
    agent.status === "running" || agent.status === "initializing" || agent.turn.phase === "open";
  const allowed = taskActions(task);
  const recent = useQuery({
    queryKey: [
      "task-board-preview",
      agent.serverId,
      agent.id,
      agent.turn.phase,
      agent.attentionTimestamp?.getTime(),
    ],
    enabled: online && Boolean(client),
    staleTime: 15_000,
    gcTime: 60_000,
    retry: false,
    refetchInterval: online && running ? 15_000 : false,
    queryFn: async () => {
      if (!client) throw new Error(t("common.errors.daemonClientUnavailable"));
      const result = await client.fetchAgentTimeline(agent.id, {
        direction: "tail",
        limit: 40,
        projection: "projected",
      });
      const latest = [...result.entries]
        .reverse()
        .find((entry) => entry.item.type === "assistant_message");
      return latest?.item.type === "assistant_message" ? latest.item.text : null;
    },
  });
  const action = useMutation({
    retry: false,
    networkMode: "always",
    mutationFn: async (kind: "stop" | "read" | "archive") => {
      if (!client || !online) throw new Error(t("taskBoard.offlineHint"));
      if (!allowed[kind]) throw new Error(t("taskBoard.actionFailed"));
      if (kind === "stop") await client.cancelAgent(agent.id);
      else if (kind === "read") await client.clearAgentAttention(agent.id);
      else {
        try {
          await archiveAgent({ serverId: agent.serverId, agentId: agent.id });
        } catch (error) {
          toast.error(toErrorMessage(error));
          throw error;
        }
      }
    },
  });
  const disabled = !online || action.isPending;
  return (
    <View style={styles.container} testID="task-detail">
      {showHeader ? (
        <View style={styles.header}>
          <Text style={styles.heading}>{t("taskBoard.details")}</Text>
          <Pressable
            onPress={onClose}
            accessibilityRole="button"
            accessibilityLabel={t("taskBoard.closeDetails")}
            testID="task-detail-close"
            hitSlop={8}
          >
            <X size={18} color={theme.colors.foregroundMuted} />
          </Pressable>
        </View>
      ) : null}
      <ScrollView contentContainerStyle={styles.content} keyboardShouldPersistTaps="handled">
        {parent ? (
          <Button variant="ghost" size="sm" leftIcon={GitFork} onPress={() => onSelect(parent.key)}>
            {parent.agent.title || t("agentList.fallbackTitle")}
          </Button>
        ) : null}
        <View style={styles.section}>
          <Text style={styles.meta}>
            {task.projectName} / {task.workspaceName}
          </Text>
          <Text style={styles.title}>{agent.title || t("agentList.fallbackTitle")}</Text>
          <TaskStatus bucket={task.ownBucket} />
          {agent.observation ? (
            <Text style={styles.meta}>
              {t("subagents.observing")}
              {agent.observation.status === "unknown"
                ? ` · ${t("subagents.observationUnknown")}`
                : ""}
              {agent.observation.health !== "current"
                ? ` · ${t("subagents.observationUnavailable")}`
                : ""}
            </Text>
          ) : null}
          <TaskProvider task={task} />
          <Text style={styles.meta}>{task.hostLabel}</Text>
          {!online ? <Text style={styles.warning}>{t("taskBoard.offlineHint")}</Text> : null}
        </View>
        {agent.lastError ? (
          <View style={styles.errorBox}>
            <Text style={styles.error}>{agent.lastError}</Text>
          </View>
        ) : null}
        {(allowed.approve ? agent.pendingPermissions : []).map((request) => (
          <View key={request.id} style={styles.approval}>
            <Text style={styles.sectionTitle}>{t("taskBoard.needsYourInput")}</Text>
            <PermissionRequestCard
              panel
              permission={{ key: request.id, agentId: agent.id, request }}
              client={online ? client : null}
            />
          </View>
        ))}
        {children.length ? (
          <View style={styles.section}>
            <View style={styles.row}>
              <GitFork size={16} color={theme.colors.foregroundMuted} />
              <Text style={styles.sectionTitle}>{t("taskBoard.childTasks")}</Text>
              <Text style={styles.meta}>{children.length}</Text>
            </View>
            {children.map((child) => (
              <Pressable
                key={child.key}
                style={[styles.child, styles.childRow]}
                onPress={() => onSelect(child.key)}
                accessibilityRole="button"
                testID={`task-child-${child.agent.id}`}
              >
                <Text style={[styles.childTitle, styles.managedChildTitle]} numberOfLines={2}>
                  {child.agent.title || t("agentList.fallbackTitle")}
                </Text>
                <TaskStatus bucket={child.bucket} />
              </Pressable>
            ))}
          </View>
        ) : null}
        {online ? <ProviderSubtasks task={task} /> : null}
        <View style={styles.section}>
          <Text style={styles.sectionTitle}>{t("taskBoard.lastReply")}</Text>
          {recent.isLoading ? (
            <LoadingSpinner size="small" color={theme.colors.foregroundMuted} />
          ) : recent.isError ? (
            <View style={styles.section}>
              <Text style={styles.meta}>{t("taskBoard.previewFailed")}</Text>
              <Button
                variant="outline"
                size="sm"
                onPress={() => void recent.refetch()}
                disabled={!online}
              >
                {t("taskBoard.retry")}
              </Button>
            </View>
          ) : (
            <Text selectable style={styles.reply} numberOfLines={16}>
              {recent.data || t("taskBoard.noReply")}
            </Text>
          )}
        </View>
      </ScrollView>
      <View style={styles.footer}>
        {action.isError ? (
          <Text accessibilityRole="alert" style={styles.error}>
            {toErrorMessage(action.error)}
          </Text>
        ) : null}
        <Button
          variant="default"
          leftIcon={ArrowUpRight}
          testID="task-open-conversation"
          onPress={() =>
            navigateToAgent({
              serverId: agent.serverId,
              agentId: agent.id,
              workspaceId: agent.workspaceId,
              pin: true,
            })
          }
        >
          {t("taskBoard.openConversation")}
        </Button>
        <View style={styles.actions}>
          {allowed.stop ? (
            <Button
              leftIcon={Square}
              size="sm"
              variant="outline"
              disabled={disabled}
              loading={action.isPending && action.variables === "stop"}
              testID="task-stop"
              onPress={() => action.mutate("stop")}
            >
              {t("taskBoard.stop")}
            </Button>
          ) : null}
          {allowed.read ? (
            <Button
              leftIcon={Check}
              size="sm"
              variant="outline"
              disabled={disabled}
              loading={action.isPending && action.variables === "read"}
              testID="task-mark-read"
              onPress={() => action.mutate("read")}
            >
              {t("taskBoard.markRead")}
            </Button>
          ) : null}
          <Button
            leftIcon={Archive}
            size="sm"
            variant="ghost"
            disabled={disabled || !allowed.archive}
            loading={action.isPending && action.variables === "archive"}
            testID="task-archive"
            onPress={() => action.mutate("archive")}
          >
            {t("taskBoard.archive")}
          </Button>
        </View>
      </View>
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  container: { flex: 1, minHeight: 0, backgroundColor: theme.colors.surface0 },
  header: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    padding: theme.spacing[4],
    borderBottomWidth: 1,
    borderBottomColor: theme.colors.border,
  },
  heading: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.xl,
    fontWeight: theme.fontWeight.semibold,
  },
  content: { padding: theme.spacing[4], gap: theme.spacing[4] },
  section: { gap: theme.spacing[2] },
  title: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize["2xl"],
    lineHeight: 28,
    fontWeight: theme.fontWeight.semibold,
  },
  sectionTitle: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.lg,
    fontWeight: theme.fontWeight.semibold,
  },
  meta: { color: theme.colors.foregroundMuted, fontSize: theme.fontSize.base, lineHeight: 21 },
  warning: { color: theme.colors.statusWarning, fontSize: theme.fontSize.sm, lineHeight: 19 },
  error: { color: theme.colors.statusDanger, fontSize: theme.fontSize.sm, lineHeight: 20 },
  errorBox: {
    padding: theme.spacing[3],
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: theme.borderRadius.lg,
  },
  approval: {
    padding: theme.spacing[4],
    borderRadius: theme.borderRadius.lg,
    borderWidth: 1,
    borderColor: theme.colors.border,
    backgroundColor: theme.colors.surface1,
    gap: theme.spacing[4],
  },
  row: { flexDirection: "row", alignItems: "center", gap: theme.spacing[2] },
  child: {
    paddingVertical: theme.spacing[3],
    paddingLeft: theme.spacing[3],
    borderLeftWidth: 2,
    borderLeftColor: theme.colors.border,
    gap: theme.spacing[2],
  },
  childTitle: { color: theme.colors.foreground, fontSize: theme.fontSize.base, lineHeight: 20 },
  childRow: { flexDirection: "row", alignItems: "center" },
  managedChildTitle: { flex: 1 },
  reply: { color: theme.colors.foregroundMuted, fontSize: theme.fontSize.base, lineHeight: 23 },
  footer: {
    padding: theme.spacing[4],
    borderTopWidth: 1,
    borderTopColor: theme.colors.border,
    gap: theme.spacing[3],
  },
  actions: {
    flexDirection: "row",
    flexWrap: "wrap",
    alignItems: "center",
    justifyContent: "center",
    gap: theme.spacing[2],
  },
}));

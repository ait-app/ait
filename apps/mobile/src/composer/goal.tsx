import { useMemo, useState } from "react";
import { Text, View } from "react-native";
import { ScrollView } from "react-native-gesture-handler";
import { useIsCompactFormFactor } from "@/constants/layout";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { Alert } from "@/components/ui/alert";
import { FormTextInput } from "@/components/ui/form-field";
import { MenuSeparator, menuRowContentInset } from "@/components/ui/menu";
import { Button } from "@/components/ui/button";
import { StatusBadge } from "@/components/ui/status-badge";
import { useSessionStore } from "@/stores/session-store";
import { useHostRuntimeClient, useHostRuntimeIsConnected } from "@/runtime/host-runtime";
import { ComposerTrackPill } from "./tracks";
import { goalCommand, readGoal, type GoalAction, type GoalSnapshot } from "./goal-model";

export function useAgentGoal(serverId: string, agentId: string) {
  const raw = useSessionStore(
    (state) =>
      (
        state.sessions[serverId]?.agents.get(agentId) ??
        state.sessions[serverId]?.agentDetails.get(agentId)
      )?.runtimeInfo?.extra?.goal,
  );
  return useMemo(() => readGoal(raw), [raw]);
}

export function AgentGoalTrack({
  serverId,
  agentId,
  goal,
}: {
  serverId: string;
  agentId: string;
  goal: GoalSnapshot | null;
}) {
  const client = useHostRuntimeClient(serverId);
  const connected = useHostRuntimeIsConnected(serverId);
  if (!goal) return null;
  return (
    <GoalTrack
      key={`${serverId}:${agentId}`}
      goal={goal}
      disabled={!client || !connected}
      onAction={async (action, objective) => {
        if (client) await client.sendMessage(agentId, goalCommand(action, objective));
      }}
    />
  );
}

export function GoalTrack({
  goal,
  disabled = false,
  onAction,
}: {
  goal: GoalSnapshot;
  disabled?: boolean;
  onAction: (action: GoalAction, objective?: string) => Promise<void>;
}) {
  const { t } = useTranslation();
  const compact = useIsCompactFormFactor();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [editBase, setEditBase] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const status = t(`agentControls.goal.status.${goal.status}`);
  const segments = useMemo(
    () => [{ bucket: null, text: t("agentControls.goal.pill", { status }) }],
    [t, status],
  );
  const hasActions = editing || Object.values(goal.actions).some(Boolean);
  const changed = editing && editBase !== goal.objective;
  const run = async (action: GoalAction) => {
    if (
      busy ||
      disabled ||
      !goal.actions[action] ||
      (action === "edit" && (!draft.trim() || changed))
    )
      return;
    setBusy(true);
    setError(null);
    try {
      await onAction(action, action === "edit" ? draft : undefined);
      setEditing(false);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("agentControls.goal.failed"));
    } finally {
      setBusy(false);
    }
  };
  return (
    <ComposerTrackPill
      testID="agent-goal-pill"
      segments={segments}
      panelTitle={t("agentControls.goal.title")}
      accessibilityLabel={t("agentControls.goal.pill", { status })}
    >
      <View testID="agent-goal-card">
        <View style={styles.metadata(compact)} testID="goal-metadata">
          <StatusBadge
            label={status}
            variant={
              goal.status === "active" || goal.status === "completed"
                ? "success"
                : goal.status === "failed"
                  ? "error"
                  : ["blocked", "budgetLimited", "usageLimited"].includes(goal.status)
                    ? "warning"
                    : "muted"
            }
          />
          <View style={styles.usage}>
            {goal.tokensUsed !== undefined ? (
              <Text style={styles.usageText}>
                {t("agentControls.goal.tokensUsed", { count: goal.tokensUsed })}
              </Text>
            ) : null}
            {goal.tokenBudget !== undefined ? (
              <Text style={styles.usageText}>
                {t("agentControls.goal.tokenBudget", { count: goal.tokenBudget })}
              </Text>
            ) : null}
          </View>
        </View>
        <View style={styles.body}>
          {editing ? (
            <FormTextInput
              testID="goal-objective-input"
              accessibilityLabel={t("agentControls.goal.objective")}
              initialValue={draft}
              onChangeText={setDraft}
              multiline
              maxLength={65536}
              editable={!busy && !disabled}
              style={styles.editor}
            />
          ) : (
            <ScrollView
              testID="goal-objective-scroll"
              style={styles.scroll}
              nestedScrollEnabled
              disallowInterruption
            >
              <Text testID="goal-objective" selectable style={styles.objective}>
                {goal.objective}
              </Text>
            </ScrollView>
          )}
          {changed || error ? (
            <Alert
              variant="error"
              description={changed ? t("agentControls.goal.changed") : error}
            />
          ) : null}
        </View>
        {hasActions ? (
          <>
            <MenuSeparator />
            <View style={styles.actions} testID="goal-actions">
              {editing ? (
                <View style={styles.editActions}>
                  <Button
                    variant="ghost"
                    size="sm"
                    style={styles.action(compact)}
                    disabled={busy}
                    onPress={() => {
                      setEditing(false);
                      setError(null);
                    }}
                  >
                    {t("agentControls.goal.cancel")}
                  </Button>
                  <Button
                    variant="default"
                    size="sm"
                    style={styles.action(compact)}
                    disabled={disabled || busy || changed || !draft.trim()}
                    onPress={() => void run("edit")}
                  >
                    {t("agentControls.goal.save")}
                  </Button>
                </View>
              ) : (
                <>
                  {goal.actions.edit ? (
                    <Button
                      variant="secondary"
                      size="sm"
                      style={styles.action(compact)}
                      disabled={disabled || busy}
                      onPress={() => {
                        setError(null);
                        setDraft(goal.objective);
                        setEditBase(goal.objective);
                        setEditing(true);
                      }}
                    >
                      {t("agentControls.goal.edit")}
                    </Button>
                  ) : null}
                  {(["pause", "resume"] as const)
                    .filter((action) => goal.actions[action])
                    .map((action) => (
                      <Button
                        key={action}
                        variant="outline"
                        size="sm"
                        style={styles.action(compact)}
                        disabled={disabled || busy}
                        onPress={() => void run(action)}
                      >
                        {t(`agentControls.goal.${action}`)}
                      </Button>
                    ))}
                  {goal.actions.clear ? (
                    <Button
                      variant="ghost"
                      size="sm"
                      style={styles.clearAction(compact)}
                      textStyle={styles.destructiveText}
                      disabled={disabled || busy}
                      onPress={() => void run("clear")}
                    >
                      {t("agentControls.goal.clear")}
                    </Button>
                  ) : null}
                </>
              )}
            </View>
          </>
        ) : null}
      </View>
    </ComposerTrackPill>
  );
}

const styles = StyleSheet.create((theme) => ({
  metadata: (compact: boolean) => ({
    flexDirection: "row",
    flexWrap: "wrap",
    alignItems: "center",
    justifyContent: "space-between",
    gap: theme.spacing[2],
    paddingHorizontal: menuRowContentInset(theme),
    paddingTop: compact ? theme.spacing[1] : theme.spacing[2],
    paddingBottom: theme.spacing[2],
  }),
  usage: { flexShrink: 1, gap: theme.spacing[1], alignItems: "flex-end" },
  usageText: {
    fontFamily: theme.fontFamily.ui,
    fontSize: theme.fontSize.sm,
    fontWeight: theme.fontWeight.normal,
    color: theme.colors.foregroundMuted,
    fontVariant: ["tabular-nums"],
    textAlign: "right",
  },
  body: {
    paddingHorizontal: menuRowContentInset(theme),
    paddingBottom: theme.spacing[3],
    gap: theme.spacing[2],
  },
  scroll: { maxHeight: 240 },
  objective: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    fontFamily: theme.fontFamily.ui,
    fontWeight: theme.fontWeight.normal,
    lineHeight: Math.round(theme.fontSize.base * 1.55),
  },
  editor: {
    height: 200,
    textAlignVertical: "top",
    fontFamily: theme.fontFamily.ui,
    lineHeight: Math.round(theme.fontSize.base * 1.55),
  },
  actions: {
    flexDirection: "row",
    flexWrap: "wrap",
    alignItems: "center",
    gap: theme.spacing[2],
    paddingHorizontal: menuRowContentInset(theme),
    paddingVertical: theme.spacing[2],
  },
  editActions: {
    flexDirection: "row",
    justifyContent: "flex-end",
    flexGrow: 1,
    gap: theme.spacing[2],
  },
  action: (compact: boolean) => ({ minHeight: compact ? 44 : 32 }),
  clearAction: (compact: boolean) => ({
    minHeight: compact ? 44 : 32,
    marginLeft: "auto",
    paddingHorizontal: theme.spacing[2],
    marginRight: -theme.spacing[2],
  }),
  destructiveText: { color: theme.colors.destructive },
}));

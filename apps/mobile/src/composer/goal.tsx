import { useMemo, useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet, useUnistyles, withUnistyles } from "react-native-unistyles";
import { Check, Pause, Pencil, Play, Trash2, X } from "lucide-react-native";
import { Alert } from "@/components/ui/alert";
import { FormTextInput } from "@/components/ui/form-field";
import { MenuHint, MenuItem, MenuLabel } from "@/components/ui/menu";
import { destructiveIconColorMapping, mutedIconColorMapping } from "@/components/ui/icon-color";
import { useSessionStore } from "@/stores/session-store";
import { useHostRuntimeClient, useHostRuntimeIsConnected } from "@/runtime/host-runtime";
import { ComposerTrackActions, ComposerTrackPill, ComposerTrackRow } from "./tracks";
import { goalCommand, readGoal, type GoalAction, type GoalSnapshot } from "./goal-model";

const ThemedCheck = withUnistyles(Check);
const ThemedPause = withUnistyles(Pause);
const ThemedPencil = withUnistyles(Pencil);
const ThemedPlay = withUnistyles(Play);
const ThemedTrash = withUnistyles(Trash2);
const ThemedX = withUnistyles(X);

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
  const { theme } = useUnistyles();
  const objectiveStyle = useMemo(
    () => ({
      color: theme.colors.foreground,
      fontSize: theme.fontSize.base,
      fontFamily: theme.fontFamily.ui,
    }),
    [theme.colors.foreground, theme.fontSize.base, theme.fontFamily.ui],
  );
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
        <ComposerTrackActions>
          {editing ? (
            <>
              <MenuItem
                leading={<ThemedCheck size={16} uniProps={mutedIconColorMapping} />}
                disabled={disabled || busy || changed || !draft.trim()}
                closeOnSelect={false}
                onSelect={() => void run("edit")}
              >
                {t("agentControls.goal.save")}
              </MenuItem>
              <MenuItem
                leading={<ThemedX size={16} uniProps={mutedIconColorMapping} />}
                disabled={busy}
                closeOnSelect={false}
                onSelect={() => setEditing(false)}
              >
                {t("agentControls.goal.cancel")}
              </MenuItem>
            </>
          ) : (
            <>
              {goal.actions.edit ? (
                <MenuItem
                  leading={<ThemedPencil size={16} uniProps={mutedIconColorMapping} />}
                  disabled={disabled || busy}
                  closeOnSelect={false}
                  onSelect={() => {
                    setDraft(goal.objective);
                    setEditBase(goal.objective);
                    setEditing(true);
                  }}
                >
                  {t("agentControls.goal.edit")}
                </MenuItem>
              ) : null}
              {(["pause", "resume", "clear"] as const)
                .filter((action) => goal.actions[action])
                .map((action) => (
                  <MenuItem
                    key={action}
                    leading={
                      action === "clear" ? (
                        <ThemedTrash size={16} uniProps={destructiveIconColorMapping} />
                      ) : action === "pause" ? (
                        <ThemedPause size={16} uniProps={mutedIconColorMapping} />
                      ) : (
                        <ThemedPlay size={16} uniProps={mutedIconColorMapping} />
                      )
                    }
                    destructive={action === "clear"}
                    disabled={disabled || busy}
                    closeOnSelect={false}
                    onSelect={() => void run(action)}
                  >
                    {t(`agentControls.goal.${action}`)}
                  </MenuItem>
                ))}
            </>
          )}
        </ComposerTrackActions>
        <MenuLabel>{status}</MenuLabel>
        <ComposerTrackRow>
          <View style={styles.content}>
            {editing ? (
              <FormTextInput
                testID="goal-objective-input"
                accessibilityLabel={t("agentControls.goal.objective")}
                initialValue={draft}
                onChangeText={setDraft}
                multiline
                maxLength={65536}
                editable={!busy && !disabled}
              />
            ) : (
              <Text selectable style={objectiveStyle}>
                {goal.objective}
              </Text>
            )}
          </View>
        </ComposerTrackRow>
        {goal.tokensUsed !== undefined ? (
          <MenuHint>{t("agentControls.goal.tokensUsed", { count: goal.tokensUsed })}</MenuHint>
        ) : null}
        {goal.tokenBudget !== undefined ? (
          <MenuHint>{t("agentControls.goal.tokenBudget", { count: goal.tokenBudget })}</MenuHint>
        ) : null}
        {changed || error ? (
          <ComposerTrackRow>
            <View style={styles.content}>
              <Alert
                variant="error"
                description={changed ? t("agentControls.goal.changed") : error}
              />
            </View>
          </ComposerTrackRow>
        ) : null}
      </View>
    </ComposerTrackPill>
  );
}

const styles = StyleSheet.create({
  content: { flexGrow: 1, flexShrink: 1, minWidth: 0 },
});

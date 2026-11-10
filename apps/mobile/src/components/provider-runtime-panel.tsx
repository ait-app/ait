import { useEffect, useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { Button } from "@/components/ui/button";
import { useBtwVisibilityStore } from "@/stores/btw-visibility-store";
import { useSessionStore } from "@/stores/session-store";
import { openExternalUrl } from "@/utils/open-external-url";
import { ProviderSideQuestions } from "./provider-side-questions";
import { ProviderCommandCard } from "./provider-command-card";
import { ProviderControlDock, type ProviderDockItem } from "./provider-control-dock";
import { ProviderVibe } from "./provider-vibe";
import { useHostRuntimeClient } from "@/runtime/host-runtime";

// Remember native editor deliveries across navigation without caching their text or auth URLs.
const appliedEditors = new Map<string, string>();

export function ProviderRuntimePanel({
  serverId,
  agentId,
  placement,
  blocked = false,
  onEditor,
}: {
  serverId: string;
  agentId: string;
  placement: "aboveEditor" | "belowEditor";
  blocked?: boolean;
  onEditor?: (text: string) => void | (() => void);
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const state = useSessionStore(
    (store) => store.sessions[serverId]?.agents.get(agentId)?.providerUi,
  );
  const mainStatus = useSessionStore(
    (store) => store.sessions[serverId]?.agents.get(agentId)?.status,
  );
  const [error, setError] = useState<string | null>(null);
  const [recovery, setRecovery] = useState<{ owner: string; restore: () => void } | null>(null);
  const owner = `${serverId}:${agentId}`;
  const canRestore = recovery?.owner === owner;
  const btwHydrated = useBtwVisibilityStore((store) => store.hydrated);
  const endedBtw = useBtwVisibilityStore(
    (store) => store.ended.find(([key]) => key === owner)?.[1],
  );
  const editor = state?.editor;
  useEffect(() => {
    if (!editor || !onEditor || blocked) return;
    const key = `${serverId}:${agentId}`;
    if (appliedEditors.get(key) === editor.id) return;
    const restore = onEditor(editor.text);
    setRecovery(restore ? { owner: key, restore } : null);
    appliedEditors.delete(key);
    appliedEditors.set(key, editor.id);
    if (appliedEditors.size > 256) appliedEditors.delete(appliedEditors.keys().next().value!);
  }, [serverId, agentId, editor, onEditor, blocked]);
  if (!state) return null;
  const above = placement === "aboveEditor";
  const activeVibe = above && Boolean(state.vibe?.enabled);
  const hasSideQuestions = Boolean(
    btwHydrated && state.sideQuestions?.reveal && state.sideQuestions.reveal !== endedBtw,
  );
  const widgets = state.widgets.filter((widget) => widget.placement === placement);
  const controls: ProviderDockItem[] = [];
  const goal = state.goal;
  if (above && goal?.goal) {
    controls.push({
      id: `goal:${goal.goal.id}`,
      label: `${t("providerControl.goal")} · ${t(`providerControl.status.${goal.goal.phase}`)}`,
      content: (
        <ProviderCommandCard
          key={goal.goal.id}
          result={goal}
          active={!blocked && Boolean(client)}
          compact
          onCommand={async (command) => {
            if (!client) throw new Error(t("workspace.terminal.hostDisconnected"));
            const current = useSessionStore.getState().sessions[serverId]?.agents.get(agentId)
              ?.providerUi?.goal;
            if (!current?.goal || current.goal.id !== goal.goal!.id)
              throw new Error(t("providerControl.goalChanged"));
            if ("provider" in goal.goal! && goal.goal.provider === "omp") {
              const action = goal.actions?.find((item) => item.command === command);
              const op =
                action?.id === "pause"
                  ? "pause"
                  : action?.id === "resume"
                    ? "resume"
                    : action?.id === "clear"
                      ? "drop"
                      : null;
              if (!op) throw new Error(t("providerControl.goalChanged"));
              await client.runNativeAction(agentId, "goal", { op, expectedGoalId: goal.goal!.id });
            } else {
              await client.sendMessage(agentId, command);
            }
          }}
        />
      ),
    });
  }
  if (above && hasSideQuestions && state.sideQuestions)
    controls.push({
      id: "side-questions",
      label: t("providerControl.sideQuestions.title"),
      content: ({ expanded, revealEnd }) => (
        <ProviderSideQuestions
          expanded={expanded}
          onInputFocus={revealEnd}
          branchAvailable={Boolean(state.actions?.some((action) => action.id === "btw_branch"))}
          branchDisabled={
            mainStatus === "running" ||
            mainStatus === "initializing" ||
            Boolean(state.vibe?.enabled)
          }
          onEnd={() => useBtwVisibilityStore.getState().end(owner, state.sideQuestions!.reveal!)}
          state={state.sideQuestions!}
          serverId={serverId}
          agentId={agentId}
          disabled={blocked}
        />
      ),
    });
  if (above)
    for (const status of state.statuses)
      controls.push({
        id: `status:${status.key}`,
        label: status.text.split("\n")[0]!.slice(0, 80),
        content: (
          <Text style={styles.text} selectable>
            {status.text}
          </Text>
        ),
      });
  for (const widget of widgets)
    controls.push({
      id: `widget:${widget.key}`,
      label: widget.lines[0]?.slice(0, 80) || t("providerControl.expandWidget"),
      content: (
        <Text style={styles.text} selectable>
          {widget.lines.join("\n")}
        </Text>
      ),
    });
  if (!controls.length && !activeVibe && (!above || (!state.title && !state.link && !canRestore)))
    return null;
  return (
    <View style={styles.panel} testID={`provider-runtime-${placement}`}>
      {above && state.title ? <Text style={styles.title}>{state.title}</Text> : null}
      <ProviderControlDock
        key={`${owner}:${placement}`}
        owner={`${owner}:${placement}`}
        items={controls}
        initial={controls[0]?.id}
        reveal={
          above && hasSideQuestions && state.sideQuestions?.reveal
            ? { id: "side-questions", revision: state.sideQuestions.reveal }
            : null
        }
        status={activeVibe && state.vibe ? <ProviderVibe state={state.vibe} /> : null}
      />
      {above && canRestore ? (
        <Button
          variant="ghost"
          size="sm"
          testID="provider-editor-restore"
          onPress={() => {
            recovery?.restore();
            setRecovery(null);
          }}
        >
          {t("providerControl.restoreDraft")}
        </Button>
      ) : null}
      {above && state.link ? (
        <View style={styles.link}>
          {state.link.instructions ? (
            <Text style={styles.text} selectable>
              {state.link.instructions}
            </Text>
          ) : null}
          <Button
            variant="outline"
            size="sm"
            onPress={() => {
              setError(null);
              void openExternalUrl(state.link!.launchUrl).catch((cause: unknown) =>
                setError(String(cause)),
              );
            }}
          >
            <Text style={styles.text}>{t("providerControl.openBrowser")}</Text>
          </Button>
        </View>
      ) : null}
      {error ? (
        <Text accessibilityRole="alert" style={styles.error}>
          {error}
        </Text>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  panel: { gap: 6, paddingHorizontal: 12, paddingVertical: 8 },
  link: { gap: 8, alignItems: "flex-start" },
  title: { color: theme.colors.foreground, fontSize: 13, fontWeight: "600" },
  text: { color: theme.colors.foregroundMuted, fontSize: 12, lineHeight: 18 },
  error: { color: theme.colors.statusDanger, fontSize: 12 },
}));

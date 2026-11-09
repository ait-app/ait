import { useCallback, useEffect, useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { Button } from "@/components/ui/button";
import { useSessionStore } from "@/stores/session-store";
import { openExternalUrl } from "@/utils/open-external-url";
import { ProviderSideQuestions } from "./provider-side-questions";
import { hasActiveVibeWorkers, ProviderVibe } from "./provider-vibe";
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
  onEditor?: (text: string) => void;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const exitVibe = useCallback(async () => {
    if (!client) throw new Error(t("workspace.terminal.hostDisconnected"));
    await client.runNativeAction(agentId, "vibe_control", {
      operation: "toggle",
      enabled: "false",
    });
  }, [agentId, client, t]);
  const state = useSessionStore(
    (store) => store.sessions[serverId]?.agents.get(agentId)?.providerUi,
  );
  const [error, setError] = useState<string | null>(null);
  const editor = state?.editor;
  useEffect(() => {
    if (!editor || !onEditor || blocked) return;
    const key = `${serverId}:${agentId}`;
    if (appliedEditors.get(key) === editor.id) return;
    onEditor(editor.text);
    appliedEditors.delete(key);
    appliedEditors.set(key, editor.id);
    if (appliedEditors.size > 256) appliedEditors.delete(appliedEditors.keys().next().value!);
  }, [serverId, agentId, editor, onEditor, blocked]);
  if (!state) return null;
  const above = placement === "aboveEditor";
  const activeVibe = hasActiveVibeWorkers(state.vibe);
  const hasSideQuestions = Boolean(
    state.sideQuestions &&
    (state.sideQuestions.records.length || state.sideQuestions.error || state.sideQuestions.reveal),
  );
  const widgets = state.widgets.filter((widget) => widget.placement === placement);
  if (
    !widgets.length &&
    (!above ||
      (!state.title && !state.statuses.length && !state.link && !hasSideQuestions && !activeVibe))
  )
    return null;
  return (
    <View style={styles.panel} testID={`provider-runtime-${placement}`}>
      {above && state.title ? <Text style={styles.title}>{state.title}</Text> : null}
      {above && state.sideQuestions ? (
        <ProviderSideQuestions
          key={`${serverId}:${agentId}`}
          state={state.sideQuestions}
          serverId={serverId}
          agentId={agentId}
          disabled={blocked}
        />
      ) : null}
      {above && activeVibe && state.vibe ? (
        <ProviderVibe
          key={`vibe:${serverId}:${agentId}`}
          state={state.vibe}
          onExit={exitVibe}
          disabled={blocked || !client}
        />
      ) : null}
      {above &&
        state.statuses.map((status) => (
          <Text key={status.key} style={styles.text} selectable>
            {status.text}
          </Text>
        ))}
      {widgets.map((widget) => (
        <View key={widget.key} testID={`provider-widget-${widget.key}`}>
          <Text style={styles.text} selectable>
            {widget.lines.join("\n")}
          </Text>
        </View>
      ))}
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

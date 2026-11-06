import { useEffect, useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { Button } from "@/components/ui/button";
import { useSessionStore } from "@/stores/session-store";
import { openExternalUrl } from "@/utils/open-external-url";
import { ProviderNativeActions } from "./provider-native-actions";
import { ProviderSideQuestions } from "./provider-side-questions";

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
  const state = useSessionStore(
    (store) => store.sessions[serverId]?.agents.get(agentId)?.providerUi,
  );
  const cwd = useSessionStore((store) => store.sessions[serverId]?.agents.get(agentId)?.cwd);
  const workspaceId = useSessionStore(
    (store) => store.sessions[serverId]?.agents.get(agentId)?.workspaceId,
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
  const widgets = state.widgets.filter((widget) => widget.placement === placement);
  if (
    !widgets.length &&
    (!above ||
      (!state.title &&
        !state.statuses.length &&
        !state.link &&
        !state.actions?.length &&
        !state.sideQuestions))
  )
    return null;
  return (
    <View style={styles.panel} testID={`provider-runtime-${placement}`}>
      {above && state.actions?.length && cwd ? (
        <ProviderNativeActions
          key={`${serverId}:${agentId}`}
          serverId={serverId}
          agentId={agentId}
          cwd={cwd}
          workspaceId={workspaceId}
          actions={state.actions}
          disabled={blocked}
        />
      ) : null}
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

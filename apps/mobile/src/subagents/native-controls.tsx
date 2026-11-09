import { useState } from "react";
import { Text, View } from "react-native";
import { MessageSquare, Square } from "lucide-react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { AdaptiveModalSheet, AdaptiveTextInput } from "@/components/adaptive-modal-sheet";
import { Button } from "@/components/ui/button";
import { ToolbarButton, paneContentToolbarIconSize } from "@/components/ui/pane-content-toolbar";
import { useSessionStore } from "@/stores/session-store";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import type { SubagentRow } from "./select";

/** Provider-owned tasks are controlled through their parent writer, never their read-only view. */
export function NativeSubagentControls({
  serverId,
  row,
  compact,
}: {
  serverId: string;
  row: SubagentRow;
  compact: boolean;
}) {
  const { t } = useTranslation();
  const observation = row.kind === "paseo" ? row.observation : undefined;
  const parentId = row.kind === "provider" ? row.parentAgentId : observation?.rootAgentId;
  const parent = useSessionStore((state) =>
    parentId ? state.sessions[serverId]?.agents.get(parentId) : undefined,
  );
  const client = useHostRuntimeClient(serverId);
  const vibe = parent?.providerUi?.vibe;
  const worker = vibe?.workers.find(
    (item) => item.id === row.id || item.id === observation?.nativeId,
  );
  const actions = parent?.providerUi?.actions;
  const nativeId = observation?.nativeId ?? row.id;
  const healthy = !observation || observation.health === "current";
  const canGuide =
    healthy &&
    (worker
      ? vibe?.enabled && actions?.some((item) => item.id === "vibe_control")
      : actions?.some((item) => item.id === "steer_subagent"));
  const canStop =
    healthy &&
    (worker
      ? vibe?.enabled && ["running", "unknown"].includes(worker.status)
      : actions?.some((item) => item.id === "cancel_subagent") && row.status === "running");
  const [mode, setMode] = useState<"guide" | "stop" | null>(null);
  const [message, setMessage] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const label = row.title ?? row.description ?? t("providerControl.tasks.worker");
  const close = () => {
    if (!pending) {
      setMode(null);
      setError(null);
    }
  };
  const run = async () => {
    if (
      !client ||
      !parentId ||
      pending ||
      (mode === "guide" ? !canGuide || !message.trim() : !canStop)
    )
      return;
    setPending(true);
    setError(null);
    try {
      await client.runNativeAction(
        parentId,
        worker ? "vibe_control" : mode === "guide" ? "steer_subagent" : "cancel_subagent",
        worker
          ? {
              operation: mode === "guide" ? "send" : "kill",
              sessionId: worker.id,
              ...(mode === "guide" ? { message: message.trim() } : {}),
            }
          : { subagentId: nativeId, ...(mode === "guide" ? { message: message.trim() } : {}) },
      );
      setNotice(
        t(mode === "guide" ? "providerControl.tasks.sent" : "providerControl.tasks.stopped"),
      );
      setMessage("");
      setMode(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setPending(false);
    }
  };
  if (!parentId || !client || (!canGuide && !canStop)) return null;
  const iconSize = paneContentToolbarIconSize(compact);
  return (
    <View style={styles.controls}>
      {canGuide ? (
        <ToolbarButton
          label={t("providerControl.tasks.guide")}
          compact={compact}
          testID={`native-guide-${row.id}`}
          onPress={(event) => {
            event.stopPropagation();
            setMode("guide");
            setError(null);
          }}
        >
          <MessageSquare size={iconSize} color={styles.caption.color} />
        </ToolbarButton>
      ) : null}
      {canStop ? (
        <ToolbarButton
          label={t("providerControl.tasks.stop")}
          compact={compact}
          testID={`native-stop-${row.id}`}
          onPress={(event) => {
            event.stopPropagation();
            setMode("stop");
            setError(null);
          }}
        >
          <Square size={iconSize} color={styles.caption.color} />
        </ToolbarButton>
      ) : null}
      {notice ? (
        <Text accessibilityLiveRegion="polite" style={styles.caption}>
          {notice}
        </Text>
      ) : null}
      {mode ? (
        <AdaptiveModalSheet
          visible
          onClose={close}
          header={{
            title:
              mode === "stop"
                ? t("providerControl.tasks.stopTitle", { name: label })
                : `${t("providerControl.tasks.guide")} · ${label}`,
          }}
          testID={`native-worker-${mode}`}
          footer={
            <View style={styles.footer}>
              <Button variant="secondary" size="sm" disabled={pending} onPress={close}>
                {t("common.actions.cancel")}
              </Button>
              <Button
                size="sm"
                variant={mode === "stop" ? "destructive" : "default"}
                loading={pending}
                disabled={pending || (mode === "guide" ? !message.trim() || !canGuide : !canStop)}
                onPress={() => void run()}
                testID="native-worker-confirm"
              >
                {t(
                  mode === "guide"
                    ? "providerControl.operations.send"
                    : "providerControl.operations.kill",
                )}
              </Button>
            </View>
          }
        >
          <View style={styles.form}>
            {mode === "guide" ? (
              <AdaptiveTextInput
                initialValue={message}
                onChangeText={setMessage}
                multiline
                editable={!pending}
                placeholder={t("providerControl.tasks.guidance")}
                accessibilityLabel={t("providerControl.tasks.guidance")}
                style={styles.input}
                testID="native-worker-guidance"
              />
            ) : (
              <Text style={styles.caption}>{label}</Text>
            )}
            {error ? (
              <Text accessibilityRole="alert" style={styles.error}>
                {error}
              </Text>
            ) : null}
          </View>
        </AdaptiveModalSheet>
      ) : null}
    </View>
  );
}
const styles = StyleSheet.create((theme) => ({
  controls: { flexDirection: "row", alignItems: "center", gap: 4 },
  footer: { flexDirection: "row", justifyContent: "flex-end", gap: 8 },
  form: { gap: 12 },
  caption: { color: theme.colors.foregroundMuted, fontSize: 12 },
  input: {
    minHeight: 110,
    maxHeight: 220,
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: 8,
    padding: 12,
    color: theme.colors.foreground,
  },
  error: { color: theme.colors.statusDanger, fontSize: 12 },
}));

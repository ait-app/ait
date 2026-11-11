import { useCallback, useState, type ReactElement } from "react";
import { Text, View } from "react-native";
import { MessageSquare, Square } from "lucide-react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { AdaptiveModalSheet, AdaptiveTextInput } from "@/components/adaptive-modal-sheet";
import { Button } from "@/components/ui/button";
import { ToolbarButton, paneContentToolbarIconSize } from "@/components/ui/pane-content-toolbar";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { useSessionStore, type Agent } from "@/stores/session-store";
import type { SubagentRow } from "./select";

/** Receipt wording; acceptance and the not-running fact are different messages. */
const NOTICE_KEYS = {
  guide_sent: "providerControl.tasks.sent",
  guide_not_running: "providerControl.tasks.notRunningGuide",
  stop_accepted: "providerControl.tasks.stopped",
  stop_not_running: "providerControl.tasks.notRunningStop",
} as const;

type Notice =
  | { kind: "guide_sent" }
  | { kind: "guide_not_running" }
  | { kind: "stop_accepted" }
  | { kind: "stop_not_running" };

/**
 * Guide and stop one observed native subagent through its parent's live managed writer.
 *
 * Controls act on Ait identities only: the child stays a read-only observation, the parent's
 * adapter resolves the native identity, and the typed receipt distinguishes acceptance from the
 * fact that nothing was running. A child that is nested under another observed subagent is not
 * offered control, because the owning writer is that intermediate parent, not this root.
 */
export function NativeSubagentControls({
  serverId,
  row,
  compact,
}: {
  serverId: string;
  row: SubagentRow;
  compact: boolean;
}): ReactElement | null {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const controlsEnabled = useSessionStore(
    (state) => state.sessions[serverId]?.serverInfo?.features?.subagentControls === true,
  );
  const observation = row.kind === "paseo" ? row.observation : undefined;
  const parentAgentId = useSessionStore((state) => {
    const session = state.sessions[serverId];
    if (!observation) return null;
    const child = session?.agents.get(row.id) ?? session?.agentDetails.get(row.id);
    const direct = child?.parentAgentId ?? null;
    // The root parent owns the writer; an intermediate observed parent has none.
    return direct && direct === observation.rootAgentId ? direct : null;
  });
  const parent = useSessionStore((state): Agent | undefined =>
    parentAgentId
      ? (state.sessions[serverId]?.agents.get(parentAgentId) ??
        state.sessions[serverId]?.agentDetails.get(parentAgentId))
      : undefined,
  );
  const [mode, setMode] = useState<"guide" | "stop" | null>(null);
  const [message, setMessage] = useState("");
  const [pending, setPending] = useState(false);
  const [failed, setFailed] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);

  const close = useCallback(() => {
    if (pending) return;
    setMode(null);
    setFailed(false);
  }, [pending]);

  const run = useCallback(async () => {
    if (!client || !parentAgentId || pending || !mode) return;
    setPending(true);
    setFailed(false);
    try {
      if (mode === "guide") {
        const receipt = await client.guideSubagent(parentAgentId, row.id, message.trim());
        setNotice(
          receipt.disposition === "accepted"
            ? { kind: "guide_sent" }
            : { kind: "guide_not_running" },
        );
        setMessage("");
      } else {
        const receipt = await client.stopSubagent(parentAgentId, row.id);
        setNotice(
          receipt.disposition === "cancelled"
            ? { kind: "stop_accepted" }
            : { kind: "stop_not_running" },
        );
      }
      setMode(null);
    } catch {
      // Server refusals are already classified; the raw native/daemon detail never reaches the row.
      setFailed(true);
    } finally {
      setPending(false);
    }
  }, [client, message, mode, parentAgentId, pending, row.id]);

  if (!controlsEnabled || !observation || !parentAgentId || !parent || !client) return null;
  // Only a managed, running parent owns a writer this client can reach.
  if (parent.management === "observed" || parent.status === "closed") return null;
  const current = observation.health === "current";
  const running = observation.status === "running";
  const canGuide = current && running && parent.capabilities.supportsChildGuide === true;
  const canStop = current && running && parent.capabilities.supportsChildStop === true;
  if (!canGuide && !canStop) return null;

  const iconSize = paneContentToolbarIconSize(compact);
  const label = row.title ?? row.description ?? t("providerControl.tasks.worker");
  // The receipt states what the parent accepted; the observation, not this notice, states the
  // child's current lifecycle, and a terminal observation hides these controls altogether.
  const noticeText = notice ? t(NOTICE_KEYS[notice.kind]) : null;

  return (
    <View style={styles.controls}>
      {canGuide ? (
        <ToolbarButton
          label={t("providerControl.tasks.guide")}
          tooltip={
            <Text style={styles.tooltip}>{t("providerControl.tasks.guideAction", { label })}</Text>
          }
          compact={compact}
          testID={`native-guide-${row.id}`}
          onPress={(event) => {
            event.stopPropagation();
            setFailed(false);
            setMode("guide");
          }}
        >
          <MessageSquare size={iconSize} color={styles.glyph.color} />
        </ToolbarButton>
      ) : null}
      {canStop ? (
        <ToolbarButton
          label={t("providerControl.tasks.stop")}
          tooltip={
            <Text style={styles.tooltip}>{t("providerControl.tasks.stopAction", { label })}</Text>
          }
          compact={compact}
          testID={`native-stop-${row.id}`}
          onPress={(event) => {
            event.stopPropagation();
            setFailed(false);
            setMode("stop");
          }}
        >
          <Square size={iconSize} color={styles.glyph.color} />
        </ToolbarButton>
      ) : null}
      {noticeText ? (
        <Text
          accessibilityLiveRegion="polite"
          style={styles.notice}
          testID={`native-notice-${row.id}`}
        >
          {noticeText}
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
                : t("providerControl.tasks.guideTitle", { name: label }),
            subtitle: mode === "stop" ? t("providerControl.tasks.stopDescription") : undefined,
          }}
          testID={`native-control-sheet-${mode}`}
          footer={
            <View style={styles.footer}>
              <Button variant="secondary" size="sm" disabled={pending} onPress={close}>
                {t("common.actions.cancel")}
              </Button>
              <Button
                size="sm"
                variant={mode === "stop" ? "destructive" : "default"}
                loading={pending}
                disabled={pending || (mode === "guide" && !message.trim())}
                onPress={() => void run()}
                testID="native-control-confirm"
              >
                {t(
                  mode === "stop"
                    ? "providerControl.tasks.confirmStop"
                    : "providerControl.tasks.confirmGuide",
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
                testID="native-control-guidance"
              />
            ) : (
              <Text style={styles.notice}>{label}</Text>
            )}
            {failed ? (
              <Text accessibilityRole="alert" style={styles.error} testID="native-control-error">
                {t(
                  mode === "stop"
                    ? "providerControl.tasks.stopFailed"
                    : "providerControl.tasks.guideFailed",
                )}
              </Text>
            ) : null}
          </View>
        </AdaptiveModalSheet>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  controls: { flexDirection: "row", alignItems: "center", gap: theme.spacing[1] },
  glyph: { color: theme.colors.foregroundMuted },
  footer: { flexDirection: "row", justifyContent: "flex-end", gap: theme.spacing[2] },
  form: { gap: theme.spacing[3] },
  notice: { fontSize: theme.fontSize.sm, color: theme.colors.foregroundMuted },
  input: {
    minHeight: 110,
    maxHeight: 220,
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: theme.borderRadius.md,
    padding: theme.spacing[3],
    color: theme.colors.foreground,
  },
  error: { fontSize: theme.fontSize.sm, color: theme.colors.statusDanger },
  tooltip: { fontSize: theme.fontSize.sm, color: theme.colors.foreground },
}));

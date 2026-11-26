import { sameAccountService } from "@ait/client/internal/account-session";
import { useCallback, useRef, useState, type ReactElement } from "react";
import { Text, View } from "react-native";
import { MessageSquare, Square } from "lucide-react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { AdaptiveModalSheet, AdaptiveTextInput } from "@/components/adaptive-modal-sheet";
import { Button } from "@/components/ui/button";
import { ToolbarButton, paneContentToolbarIconSize } from "@/components/ui/pane-content-toolbar";
import {
  useHostRuntimeClient,
  useHostRuntimeIsConnected,
  useHostRuntimeSnapshot,
  useHosts,
} from "@/runtime/host-runtime";
import { useSessionStore, type Agent } from "@/stores/session-store";
import { useAccountState, type AccountState } from "@/runtime/account-state";
import type { HostProfile } from "@/types/host-connection";
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

/** Client-side availability mirrors guest attribution; the daemon remains the authority. */
function mayDriveParent(
  host: HostProfile | undefined,
  connectionId: string | null | undefined,
  account: Pick<AccountState, "status" | "center" | "accountId">,
  parent: Agent | undefined,
): boolean {
  const connection = host?.connections.find((candidate) => candidate.id === connectionId);
  if (!connection || !parent) return false;
  if (connection.type !== "accountRelay" || !connection.shareId) return true;
  if (connection.shareResourceKind === "node") return true;
  if (connection.shareResourceKind !== "workspace" && connection.shareResourceKind !== "project") {
    return false;
  }
  return (
    account.status === "online" &&
    sameAccountService(account.center, connection.center) &&
    Boolean(account.accountId) &&
    parent.labels["ait.guest.share-id"] === connection.shareId &&
    parent.labels["ait.guest.user-id"] === account.accountId
  );
}

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
  const connected = useHostRuntimeIsConnected(serverId);
  const runtime = useHostRuntimeSnapshot(serverId);
  const account = useAccountState();
  const host = useHosts().find((candidate) => candidate.serverId === serverId);
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
  const mayDrive = mayDriveParent(host, runtime?.activeConnectionId, account, parent);
  const [mode, setMode] = useState<"guide" | "stop" | null>(null);
  const [message, setMessage] = useState("");
  const [pending, setPending] = useState(false);
  const pendingRef = useRef(false);
  const [failed, setFailed] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);

  const close = useCallback(() => {
    if (pending) return;
    setMode(null);
    setFailed(false);
  }, [pending]);

  const run = useCallback(async () => {
    if (!client || !connected || !mayDrive || !parentAgentId || pendingRef.current || !mode) return;
    pendingRef.current = true;
    setPending(true);
    setFailed(false);
    try {
      if (mode === "guide") {
        const receipt = await client.guideSubagent(parentAgentId, row.id, message, {
          expectedRevision: observation?.revision,
        });
        setNotice(
          receipt.disposition === "accepted"
            ? { kind: "guide_sent" }
            : { kind: "guide_not_running" },
        );
        setMessage("");
      } else {
        const receipt = await client.stopSubagent(parentAgentId, row.id, {
          expectedRevision: observation?.revision,
        });
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
      pendingRef.current = false;
      setPending(false);
    }
  }, [client, connected, mayDrive, message, mode, observation?.revision, parentAgentId, row.id]);

  if (
    !controlsEnabled ||
    !connected ||
    !mayDrive ||
    !observation ||
    !parentAgentId ||
    !parent ||
    !client
  )
    return null;
  // Only a managed parent with a live writer offers these capabilities; its turn may be idle.
  if (parent.management === "observed" || parent.status === "closed") return null;
  const current = observation.health === "current";
  const running = observation.status === "running";
  const canGuide = current && running && parent.capabilities.supportsChildGuide === true;
  const canStop = current && running && parent.capabilities.supportsChildStop === true;
  if (!canGuide && !canStop) return null;

  const guidanceTooLong = new TextEncoder().encode(message).byteLength > 65536;
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
                disabled={pending || (mode === "guide" && (!message.trim() || guidanceTooLong))}
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
            {mode === "guide" && guidanceTooLong ? (
              <Text accessibilityRole="alert" style={styles.error}>
                {t("providerControl.tasks.tooLong")}
              </Text>
            ) : null}
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

import { useEffect, useMemo, useState } from "react";
import { Text, View } from "react-native";
import { useHubServices } from "@/runtime/hub-service-store";
import type { AgentScreenReadySyncState } from "@/hooks/use-agent-screen-state-machine";
import { useTranslation } from "react-i18next";
import { StyleSheet, withUnistyles } from "react-native-unistyles";
import { ToastViewport, type ToastState } from "@/components/toast-host";
import { LoadingSpinner } from "@/components/ui/loading-spinner";
import { foregroundIconColorMapping } from "@/components/ui/icon-color";

const ThemedLoadingSpinner = withUnistyles(LoadingSpinner);
const keepUntilSynchronized = () => {};

export function TimelineSyncStatus({
  sync,
  toast,
  onDismiss,
}: {
  sync: AgentScreenReadySyncState | null;
  toast: ToastState | null;
  onDismiss: () => void;
}) {
  const { t } = useTranslation();
  const combinedNotice = useHubServices((state) => state.connectionRecovering);
  const [elapsed, setElapsed] = useState(false);
  const recovering = sync?.status === "reconnecting" || sync?.status === "catching_up";
  const hasRetainedContent =
    sync?.status === "reconnecting" || (sync?.status === "catching_up" && sync.ui === "status");
  useEffect(() => {
    if (!recovering) {
      setElapsed(false);
      return;
    }
    const timer = setTimeout(() => setElapsed(true), 2_000);
    return () => clearTimeout(timer);
  }, [recovering]);
  let state: "reconnecting" | "updating" | null = null;
  if (elapsed && !combinedNotice) {
    if (sync?.status === "reconnecting") state = "reconnecting";
    else if (sync?.status === "catching_up" && sync.ui === "status") state = "updating";
  }
  const label = state ? t(`agentPanel.states.${state}`) : null;
  const syncToast = useMemo<ToastState | null>(
    () =>
      state && label
        ? {
            id: state === "reconnecting" ? 1 : 2,
            content: label,
            icon: <ThemedLoadingSpinner size={18} uniProps={foregroundIconColorMapping} />,
            variant: "default",
            durationMs: null,
            testID: `agent-${state}-toast`,
          }
        : null,
    [state, label],
  );
  return (
    <>
      {hasRetainedContent ? (
        <View pointerEvents="none" style={styles.stale}>
          <Text style={styles.label}>{t("agentStream.pendingUpdate")}</Text>
        </View>
      ) : null}
      <ToastViewport
        toast={toast ?? syncToast}
        onDismiss={toast ? onDismiss : keepUntilSynchronized}
        placement="panel"
      />
    </>
  );
}

const styles = StyleSheet.create((theme) => ({
  stale: { position: "absolute", top: theme.spacing[2], right: theme.spacing[3] },
  label: { color: theme.colors.foregroundMuted, fontSize: theme.fontSize.sm },
}));

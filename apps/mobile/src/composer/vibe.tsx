import { useEffect, useMemo, useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { useIsCompactFormFactor } from "@/constants/layout";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { StatusBadge } from "@/components/ui/status-badge";
import { MenuSeparator, menuRowContentInset, useMenuContext } from "@/components/ui/menu";
import { useSessionStore } from "@/stores/session-store";
import { useHostRuntimeClient, useHostRuntimeIsConnected } from "@/runtime/host-runtime";
import { ComposerTrackPill } from "./tracks";

export function AgentVibeTrack({ serverId, agentId }: { serverId: string; agentId: string }) {
  const enabled = useSessionStore((state) => {
    const snapshot =
      state.sessions[serverId]?.agents.get(agentId) ??
      state.sessions[serverId]?.agentDetails.get(agentId);
    const vibe = snapshot?.runtimeInfo?.extra?.vibe;
    return Boolean(vibe && typeof vibe === "object" && "enabled" in vibe && vibe.enabled === true);
  });
  const client = useHostRuntimeClient(serverId);
  const connected = useHostRuntimeIsConnected(serverId);
  if (!enabled) return null;
  return (
    <VibeTrack
      key={`${serverId}:${agentId}`}
      disabled={!client || !connected}
      onExit={async () => {
        if (client) await client.sendMessage(agentId, "/vibe off");
      }}
    />
  );
}

export function VibeTrack({
  disabled = false,
  onExit,
}: {
  disabled?: boolean;
  onExit: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const segments = useMemo(() => [{ bucket: null, text: t("agentControls.vibe.pill") }], [t]);
  return (
    <ComposerTrackPill
      testID="agent-vibe-pill"
      segments={segments}
      panelTitle={t("agentControls.vibe.title")}
      accessibilityLabel={t("agentControls.vibe.accessibilityLabel")}
    >
      <VibeCard disabled={disabled} onExit={onExit} />
    </ComposerTrackPill>
  );
}

function VibeCard({ disabled, onExit }: { disabled: boolean; onExit: () => Promise<void> }) {
  const { t } = useTranslation();
  const { open } = useMenuContext("VibeCard");
  const compact = useIsCompactFormFactor();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const exit = async () => {
    if (busy || disabled) return;
    setBusy(true);
    setError(null);
    try {
      await onExit();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("agentControls.vibe.failed"));
    } finally {
      setBusy(false);
    }
  };
  useEffect(() => {
    setError(null);
  }, [open]);
  return (
    <View testID="agent-vibe-card">
      <View style={styles.metadata(compact)}>
        <StatusBadge label={t("agentControls.vibe.on")} variant="success" />
        <Text style={[styles.muted, styles.director]}>{t("agentControls.vibe.director")}</Text>
      </View>
      <View style={styles.body}>
        <Text style={styles.description}>{t("agentControls.vibe.description")}</Text>
        {error ? <Alert variant="error" description={error} /> : null}
      </View>
      <MenuSeparator />
      <View style={styles.actions}>
        <Text style={[styles.muted, styles.consequence]}>
          {t("agentControls.vibe.consequence")}
        </Text>
        <Button
          size="sm"
          variant="ghost"
          style={styles.exit(compact)}
          textStyle={styles.destructive}
          loading={busy}
          disabled={disabled || busy}
          onPress={() => void exit()}
          accessibilityState={{ busy, disabled: disabled || busy }}
        >
          {t("agentControls.vibe.exit")}
        </Button>
      </View>
    </View>
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
  body: {
    paddingHorizontal: menuRowContentInset(theme),
    paddingBottom: theme.spacing[3],
    gap: theme.spacing[2],
  },
  description: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    fontFamily: theme.fontFamily.ui,
    fontWeight: theme.fontWeight.normal,
    lineHeight: Math.round(theme.fontSize.base * 1.55),
  },
  muted: {
    fontFamily: theme.fontFamily.ui,
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
  director: { flexShrink: 1, minWidth: 0, textAlign: "right" },
  consequence: { flexShrink: 1, minWidth: 0 },
  actions: {
    flexDirection: "row",
    flexWrap: "wrap",
    alignItems: "center",
    justifyContent: "space-between",
    gap: theme.spacing[2],
    paddingHorizontal: menuRowContentInset(theme),
    paddingVertical: theme.spacing[2],
  },
  exit: (compact: boolean) => ({
    minHeight: compact ? 44 : 32,
    flexShrink: 0,
    marginLeft: "auto",
    paddingHorizontal: theme.spacing[2],
    marginRight: -theme.spacing[2],
  }),
  destructive: { color: theme.colors.destructive },
}));

import { useEffect, useMemo, useRef, useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { useIsCompactFormFactor } from "@/constants/layout";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { MenuSeparator, menuRowContentInset, useMenuContext } from "@/components/ui/menu";
import { useSessionStore } from "@/stores/session-store";
import { useHostRuntimeClient, useHostRuntimeIsConnected } from "@/runtime/host-runtime";
import type { SubagentsTrackProps } from "@/subagents/track";
import { ComposerTrackPill, ComposerTrackRow } from "./tracks";
import {
  findVibeWorkerRow,
  readVibe,
  vibeWorkersForDisplay,
  type VibeSnapshot,
} from "./vibe-model";

export function useAgentVibe(serverId: string, agentId: string) {
  const raw = useSessionStore(
    (state) =>
      (
        state.sessions[serverId]?.agents.get(agentId) ??
        state.sessions[serverId]?.agentDetails.get(agentId)
      )?.runtimeInfo?.extra?.vibe,
  );
  return useMemo(() => readVibe(raw), [raw]);
}

export function AgentVibeTrack({
  serverId,
  agentId,
  vibe,
  rows,
  onOpenSubagent,
  onOpenProviderSubagent,
  opensInSheet,
}: {
  serverId: string;
  agentId: string;
  vibe: VibeSnapshot | null;
} & Pick<
  SubagentsTrackProps,
  "rows" | "onOpenSubagent" | "onOpenProviderSubagent" | "opensInSheet"
>) {
  const client = useHostRuntimeClient(serverId);
  const connected = useHostRuntimeIsConnected(serverId);
  if (!vibe) return null;
  return (
    <VibeTrack
      key={`${serverId}:${agentId}`}
      vibe={vibeWorkersForDisplay(vibe, rows, serverId, agentId)}
      disabled={!client || !connected}
      workerLink={(id) => {
        const row = findVibeWorkerRow(id, rows, serverId, agentId);
        if (!row) return undefined;
        return {
          closeOnSelect: !opensInSheet(row),
          onPress: () => {
            if (row.kind === "provider") onOpenProviderSubagent(row.parentAgentId, row.id);
            else onOpenSubagent(row.id, row.serverId);
          },
        };
      }}
      onExit={async () => {
        if (client) await client.sendMessage(agentId, "/vibe off");
      }}
    />
  );
}

interface VibeTrackProps {
  vibe: VibeSnapshot;
  disabled?: boolean;
  onExit: () => Promise<void>;
  workerLink?: (id: string) => { onPress: () => void; closeOnSelect: boolean } | undefined;
}

export function VibeTrack({ vibe, disabled = false, onExit, workerLink }: VibeTrackProps) {
  const { t } = useTranslation();
  const segments = useMemo(() => [{ bucket: null, text: t("agentControls.vibe.title") }], [t]);
  return (
    <ComposerTrackPill
      testID="agent-vibe-pill"
      segments={segments}
      panelTitle={t("agentControls.vibe.title")}
      accessibilityLabel={t("agentControls.vibe.accessibilityLabel")}
    >
      <VibeCard vibe={vibe} disabled={disabled} onExit={onExit} workerLink={workerLink} />
    </ComposerTrackPill>
  );
}

function VibeCard({ vibe, disabled, onExit, workerLink }: VibeTrackProps) {
  const { t } = useTranslation();
  const { open } = useMenuContext("VibeCard");
  const compact = useIsCompactFormFactor();
  const pending = useRef(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const exit = async () => {
    if (pending.current || disabled) return;
    pending.current = true;
    setBusy(true);
    setError(null);
    try {
      await onExit();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("agentControls.vibe.failed"));
    } finally {
      pending.current = false;
      setBusy(false);
    }
  };
  useEffect(() => {
    setError(null);
  }, [open]);
  const stopsWork =
    !vibe.workers ||
    vibe.workers.some((worker) => worker.status === "running" || worker.status === "unknown");
  return (
    <View testID="agent-vibe-card">
      {disabled ? <Text style={styles.notice}>{t("agentControls.vibe.disconnected")}</Text> : null}
      {vibe.workers?.length ? (
        vibe.workers.map((worker) => {
          const link = workerLink?.(worker.id);
          const status = disabled && worker.status === "running" ? "unknown" : worker.status;
          const title = worker.description || t("agentControls.vibe.unnamed");
          return (
            <ComposerTrackRow
              key={worker.id}
              testID={`vibe-worker-${worker.id}`}
              accessibilityLabel={title}
              onPress={link?.onPress}
              closeOnSelect={link?.closeOnSelect}
            >
              <View style={styles.worker}>
                <View style={styles.taskLine}>
                  <Text style={styles.task} numberOfLines={2}>
                    {title}
                  </Text>
                  <Text style={[styles.status, status === "failed" ? styles.failed : undefined]}>
                    {t(`agentControls.vibe.status.${status}`)}
                  </Text>
                </View>
                <Text style={styles.model} numberOfLines={2}>
                  {worker.model || t("agentControls.model.unknown")}
                </Text>
              </View>
            </ComposerTrackRow>
          );
        })
      ) : (
        <Text style={styles.notice}>
          {t(vibe.workers ? "agentControls.vibe.empty" : "agentControls.vibe.unavailable")}
        </Text>
      )}
      {error ? (
        <View style={styles.error}>
          <Alert variant="error" description={error} />
        </View>
      ) : null}
      <MenuSeparator />
      <View style={styles.actions}>
        <Button
          size="sm"
          variant="ghost"
          style={styles.exit(compact)}
          textStyle={stopsWork ? styles.destructive : styles.exitText}
          loading={busy}
          disabled={disabled || busy}
          onPress={() => void exit()}
          accessibilityState={{ busy, disabled: disabled || busy }}
        >
          {t(stopsWork ? "agentControls.vibe.exitAndStop" : "agentControls.vibe.exit")}
        </Button>
      </View>
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  worker: { flex: 1, minWidth: 0, gap: theme.spacing[1], paddingVertical: theme.spacing[1] },
  taskLine: { flexDirection: "row", alignItems: "flex-start", gap: theme.spacing[3] },
  task: {
    flex: 1,
    minWidth: 0,
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    fontFamily: theme.fontFamily.ui,
    lineHeight: 21,
  },
  status: {
    flexShrink: 0,
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    fontFamily: theme.fontFamily.ui,
    lineHeight: 21,
  },
  model: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.base,
    fontFamily: theme.fontFamily.ui,
    lineHeight: 21,
  },
  notice: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    fontFamily: theme.fontFamily.ui,
    lineHeight: 21,
    paddingHorizontal: menuRowContentInset(theme),
    paddingVertical: theme.spacing[2],
  },
  error: { paddingHorizontal: menuRowContentInset(theme), paddingVertical: theme.spacing[2] },
  actions: {
    paddingHorizontal: theme.spacing[1],
  },
  exit: (compact: boolean) => ({
    minHeight: compact ? 44 : 32,
    justifyContent: "flex-start",
    paddingHorizontal: theme.spacing[2],
  }),
  exitText: { color: theme.colors.foreground, fontSize: theme.fontSize.base },
  destructive: { color: theme.colors.destructive, fontSize: theme.fontSize.base },
  failed: { color: theme.colors.destructive },
}));

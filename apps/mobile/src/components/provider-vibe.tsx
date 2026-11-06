import { memo } from "react";
import { Pressable, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import type { ProviderUiState } from "@ait/protocol/messages";
import { StatusBadge } from "@/components/ui/status-badge";

type Vibe = NonNullable<ProviderUiState["vibe"]>;
type Worker = Vibe["workers"][number];

const badgeVariant = (status: Worker["status"]) => {
  switch (status) {
    case "completed":
      return "success" as const;
    case "failed":
      return "error" as const;
    case "running":
      return "warning" as const;
    default:
      return "muted" as const;
  }
};

/**
 * Native Vibe workers observed for this parent session.
 *
 * Renders nothing when the host does not project `vibe` (older daemons) or when
 * Vibe is disabled; rows are read-only observations, so a worker without a
 * resolvable agent stays text instead of pretending to be openable.
 */
export const ProviderVibe = memo(function ProviderVibe({
  state,
  onOpenWorker,
}: {
  state: Vibe;
  onOpenWorker?: (worker: Worker) => void;
}) {
  const { t } = useTranslation();
  if (!state.enabled || state.workers.length === 0) return null;
  return (
    <View style={styles.panel} testID="provider-vibe">
      <View style={styles.row}>
        <Text style={styles.title}>{t("providerControl.vibe.title")}</Text>
        <StatusBadge variant="warning" label={t("providerControl.vibe.enabled")} />
      </View>
      {state.workers.map((worker) => (
        <Pressable
          key={worker.id}
          disabled={!onOpenWorker}
          onPress={() => onOpenWorker?.(worker)}
          accessibilityRole={onOpenWorker ? "button" : "text"}
          testID={`provider-vibe-worker-${worker.id}`}
          style={styles.worker}
        >
          <View style={styles.row}>
            <Text style={styles.workerId} numberOfLines={1}>
              {worker.id}
            </Text>
            <StatusBadge
              variant={badgeVariant(worker.status)}
              label={t(`providerControl.vibe.states.${worker.status}`)}
            />
          </View>
          {worker.description ? (
            <Text style={styles.description} numberOfLines={3} selectable>
              {worker.description}
            </Text>
          ) : null}
          {worker.model ? (
            <Text style={styles.caption} numberOfLines={1}>
              {worker.model}
            </Text>
          ) : null}
        </Pressable>
      ))}
    </View>
  );
});

const styles = StyleSheet.create((theme) => ({
  panel: { gap: 8 },
  row: { flexDirection: "row", alignItems: "center", gap: 8, flexWrap: "wrap" },
  title: { color: theme.colors.foreground, fontSize: 13, fontWeight: "600", flexShrink: 1 },
  worker: {
    gap: 6,
    padding: 10,
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: 10,
  },
  workerId: { color: theme.colors.foreground, fontSize: 12, flexShrink: 1 },
  description: { color: theme.colors.foreground, fontSize: 13, lineHeight: 19 },
  caption: { color: theme.colors.foregroundMuted, fontSize: 12 },
}));

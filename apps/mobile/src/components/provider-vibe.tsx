import { memo, useState } from "react";
import { Pressable, ScrollView, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import type { ProviderUiState } from "@ait/protocol/messages";
import { StatusBadge } from "@/components/ui/status-badge";
import { Button } from "@/components/ui/button";

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
 * The parent hides this panel when older daemons omit `vibe`. Otherwise the live mode
 * stays visible, including disabled and empty states. Rows are read-only, so a worker without a
 * resolvable agent stays text instead of pretending to be openable.
 *
 * A fan-out can observe hundreds of workers, so the list scrolls inside a bounded panel instead
 * of pushing the rest of the session off screen.
 */
export const ProviderVibe = memo(function ProviderVibe({
  state,
  onOpenWorker,
  onExit,
  disabled = false,
}: {
  state: Vibe;
  onOpenWorker?: (worker: Worker) => void;
  onExit?: () => Promise<void>;
  disabled?: boolean;
}) {
  const { t } = useTranslation();
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const exit = async () => {
    if (!onExit || disabled || pending) return;
    setPending(true);
    setError(null);
    try {
      await onExit();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setPending(false);
    }
  };
  return (
    <View style={styles.panel} testID="provider-vibe">
      <View style={styles.row}>
        <Text style={styles.title}>{t("providerControl.vibe.title")}</Text>
        <StatusBadge
          variant={state.enabled ? "warning" : "muted"}
          label={t(
            state.enabled ? "providerControl.vibe.enabled" : "providerControl.vibe.disabled",
          )}
        />
        {state.enabled ? (
          <Button
            variant="ghost"
            size="sm"
            disabled={disabled || pending || !onExit}
            loading={pending}
            onPress={() => void exit()}
            testID="provider-vibe-exit"
          >
            {t("providerControl.vibe.exit")}
          </Button>
        ) : null}
      </View>
      {state.enabled && state.workers.length === 0 ? (
        <Text style={styles.description} testID="provider-vibe-empty">
          {t("providerControl.vibe.empty")}
        </Text>
      ) : null}
      {error ? (
        <Text accessibilityRole="alert" style={styles.description}>
          {error}
        </Text>
      ) : null}
      {state.enabled && state.workers.length ? (
        <ScrollView
          style={styles.list}
          contentContainerStyle={styles.listContent}
          nestedScrollEnabled
        >
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
        </ScrollView>
      ) : null}
    </View>
  );
});

const styles = StyleSheet.create((theme) => ({
  panel: { gap: 8 },
  row: { flexDirection: "row", alignItems: "center", gap: 8, flexWrap: "wrap" },
  title: { color: theme.colors.foreground, fontSize: 13, fontWeight: "600", flexShrink: 1 },
  // Bounded so a large fan-out never displaces the conversation; matches the other native
  // operation lists in this panel.
  list: { maxHeight: 280 },
  listContent: { gap: 6 },
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

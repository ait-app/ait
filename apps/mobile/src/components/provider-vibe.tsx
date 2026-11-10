import { memo, useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import type { ProviderUiState } from "@ait/protocol/messages";
import { StatusBadge } from "@/components/ui/status-badge";
import { Button } from "@/components/ui/button";

type Vibe = NonNullable<ProviderUiState["vibe"]>;

export const ProviderVibe = memo(function ProviderVibe({
  state,
  onExit,
  disabled = false,
  disabledReason,
}: {
  state: Vibe;
  onExit?: () => Promise<void>;
  disabled?: boolean;
  disabledReason?: string;
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
  if (!state.enabled) return null;
  return (
    <View style={styles.panel} testID="provider-vibe">
      <View style={styles.row}>
        <Text style={styles.title}>{t("providerControl.vibe.title")}</Text>
        <StatusBadge variant="warning" label={t("providerControl.vibe.enabled")} />
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
      </View>
      {!state.workers.length ? (
        <Text style={styles.description}>{t("providerControl.vibe.empty")}</Text>
      ) : null}
      {disabledReason ? <Text style={styles.description}>{disabledReason}</Text> : null}
      {error ? (
        <Text accessibilityRole="alert" style={styles.description}>
          {error}
        </Text>
      ) : null}
    </View>
  );
});

const styles = StyleSheet.create((theme) => ({
  panel: { gap: 8 },
  row: { flexDirection: "row", alignItems: "center", gap: 8, flexWrap: "wrap" },
  title: { color: theme.colors.foreground, fontSize: 13, fontWeight: "600", flexShrink: 1 },
  description: { color: theme.colors.foreground, fontSize: 13, lineHeight: 19 },
}));

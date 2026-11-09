import { useMemo, useState } from "react";
import { ScrollView, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { Button } from "@/components/ui/button";

/** Keep JSON and host paths behind details; show native statistics without fabricating fields. */
export function ProviderNativeResult({ text, fileName }: { text: string; fileName?: string }) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const parsed = useMemo((): Record<string, unknown> | null => {
    try {
      const result: unknown = JSON.parse(text);
      return result !== null && typeof result === "object" && !Array.isArray(result)
        ? (result as Record<string, unknown>)
        : null;
    } catch {
      return null;
    }
  }, [text]);
  const tokens =
    parsed?.tokens && typeof parsed.tokens === "object"
      ? (parsed.tokens as Record<string, unknown>)
      : null;
  const context =
    parsed?.contextUsage && typeof parsed.contextUsage === "object"
      ? (parsed.contextUsage as Record<string, unknown>)
      : null;
  const summaries: { label: string; value: string }[] = [];
  const metric = (key: string, value: unknown, unit: "count" | "money" | "percent" = "count") => {
    if (typeof value !== "number" || !Number.isFinite(value) || value < 0) return;
    const formatted =
      unit === "money"
        ? `$${value.toFixed(4)}`
        : unit === "percent"
          ? `${value.toFixed(1)}%`
          : new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 }).format(value);
    summaries.push({ label: t(`providerControl.statistics.${key}`), value: formatted });
  };
  metric("tokens", tokens?.total);
  metric("input", tokens?.input);
  metric("output", tokens?.output);
  metric("cost", parsed?.cost, "money");
  metric("context", context?.percent, "percent");
  if (!fileName && !parsed)
    return (
      <Text style={styles.text} selectable>
        {text}
      </Text>
    );
  return (
    <View style={styles.result} testID="provider-native-result">
      {fileName ? (
        <Text style={styles.text} selectable>
          {fileName}
        </Text>
      ) : summaries.length ? (
        <View style={styles.summary}>
          {summaries.map((entry) => (
            <View key={entry.label} style={styles.metric}>
              <Text style={styles.caption}>{entry.label}</Text>
              <Text style={styles.text}>{entry.value}</Text>
            </View>
          ))}
        </View>
      ) : (
        <Text style={styles.text}>{t("providerControl.resultAvailable")}</Text>
      )}
      {text ? (
        <Button
          variant="ghost"
          size="sm"
          onPress={() => setExpanded(!expanded)}
          testID="provider-result-details"
        >
          {t("providerControl.details")}
        </Button>
      ) : null}
      {expanded ? (
        <ScrollView style={styles.rawScroll} nestedScrollEnabled>
          <Text style={styles.raw} selectable testID="provider-result-raw">
            {text}
          </Text>
        </ScrollView>
      ) : null}
    </View>
  );
}
const styles = StyleSheet.create((theme) => ({
  result: { gap: 8 },
  summary: { flexDirection: "row", flexWrap: "wrap", gap: 16 },
  metric: { gap: 4 },
  text: { color: theme.colors.foreground, fontSize: 13, lineHeight: 19 },
  caption: { color: theme.colors.foregroundMuted, fontSize: 12 },
  rawScroll: { maxHeight: 200 },
  raw: { color: theme.colors.foregroundMuted, fontSize: 12, lineHeight: 18 },
}));

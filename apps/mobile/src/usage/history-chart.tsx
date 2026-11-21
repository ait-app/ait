import { Text, View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { settingsStyles } from "@/styles/settings";
import { useUsageCopy } from "./copy";
import { formatLedgerCount, formatLedgerDay } from "./format";
import type { DailyTokens } from "./history-model";

const CHART_HEIGHT = 88;

/**
 * Tokens per UTC day as one bar each, scaled to the busiest day. One series in the accent colour:
 * the title names it, so there is no legend. Hovering a bar names its day and tokens; the By day
 * table below holds the same figures for every reader.
 */
export function UsageHistoryChart({
  days,
  locale,
}: {
  days: readonly DailyTokens[];
  locale: string;
}) {
  const copy = useUsageCopy().history;
  const peak = days.reduce((max, entry) => Math.max(max, entry.tokens), 0);
  if (days.length < 2 || peak === 0) return null;
  const first = days[0];
  const last = days[days.length - 1];
  return (
    <View style={[settingsStyles.card, styles.card]} testID="usage-history-chart">
      <View style={styles.header}>
        <Text style={styles.title}>{copy.tokensPerDay}</Text>
        <Text style={styles.peak}>{copy.peak(formatLedgerCount(peak, locale))}</Text>
      </View>
      <View style={styles.plot}>
        {days.map((entry) => (
          <DayBar key={entry.day} entry={entry} peak={peak} locale={locale} />
        ))}
      </View>
      <View style={styles.axis}>
        <Text style={styles.axisLabel}>{formatLedgerDay(first.day, locale)}</Text>
        <Text style={styles.axisLabel}>{formatLedgerDay(last.day, locale)}</Text>
      </View>
    </View>
  );
}

/** A full-height column, so the hover target is larger than the bar it holds. */
function DayBar({ entry, peak, locale }: { entry: DailyTokens; peak: number; locale: string }) {
  const copy = useUsageCopy().history;
  const label = `${formatLedgerDay(entry.day, locale)} · ${copy.tokens(formatLedgerCount(entry.tokens, locale))}`;
  // A day with any tokens keeps a visible sliver; a day with none draws nothing above the baseline.
  const height = entry.tokens > 0 ? Math.max(2, (entry.tokens / peak) * CHART_HEIGHT) : 0;
  return (
    <Tooltip delayDuration={0} enabledOnDesktop enabledOnMobile={false}>
      <TooltipTrigger asChild>
        <View style={styles.column} accessible accessibilityLabel={label}>
          <View style={[styles.bar, { height }]} />
        </View>
      </TooltipTrigger>
      <TooltipContent side="top">
        <Text style={styles.tooltipText}>{label}</Text>
      </TooltipContent>
    </Tooltip>
  );
}

const styles = StyleSheet.create((theme) => ({
  card: {
    gap: theme.spacing[3],
    paddingVertical: theme.spacing[3],
    paddingHorizontal: theme.spacing[4],
  },
  header: {
    flexDirection: "row",
    alignItems: "baseline",
    justifyContent: "space-between",
    gap: theme.spacing[2],
  },
  title: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
  },
  peak: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    fontVariant: ["tabular-nums"],
  },
  plot: {
    height: CHART_HEIGHT,
    flexDirection: "row",
    alignItems: "flex-end",
    borderBottomWidth: 1,
    borderBottomColor: theme.colors.border,
  },
  column: {
    flex: 1,
    height: "100%",
    justifyContent: "flex-end",
    // The 2px surface gap between neighbouring bars.
    paddingHorizontal: 1,
  },
  bar: {
    borderTopLeftRadius: 4,
    borderTopRightRadius: 4,
    backgroundColor: theme.colors.accent,
  },
  axis: {
    flexDirection: "row",
    justifyContent: "space-between",
    marginTop: -theme.spacing[1],
  },
  axisLabel: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
  tooltipText: {
    color: theme.colors.popoverForeground,
    fontSize: theme.fontSize.sm,
  },
}));

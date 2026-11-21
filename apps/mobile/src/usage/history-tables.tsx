import { useTranslation } from "react-i18next";
import { Text, View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { settingsStyles } from "@/styles/settings";
import { useUsageCopy } from "./copy";
import {
  formatCacheHitRate,
  formatLedgerCount,
  formatLedgerDate,
  formatLedgerDay,
  formatReportedCost,
} from "./format";
import { UsageHistoryChart } from "./history-chart";
import {
  cacheHitRate,
  dailyTokens,
  ledgerProviderLabel,
  totalTokens,
  type HistoryRange,
  type LedgerRow,
  type UsageHistory,
} from "./history-model";
import { UsageMessage } from "./usage-section";

/**
 * History for the hosts that answered: totals as tiles, tokens per day, then the calls by provider,
 * model and day, with notes on hosts that did not answer. `range` draws the per-day chart; a
 * 24-hour range has too few days for one.
 */
export function UsageHistoryBody({
  history,
  range,
  onRetry,
}: {
  history: UsageHistory;
  range?: HistoryRange;
  onRetry: () => void;
}) {
  const usageCopy = useUsageCopy();
  const copy = usageCopy.history;
  const { i18n } = useTranslation();
  const locale = i18n.language;
  return (
    <View style={styles.body}>
      {history.failed.length > 0 ? (
        <Alert
          variant="error"
          title={usageCopy.errorTitle}
          description={history.failed
            .map((failure) => copy.failed(failure.label, failure.message))
            .join("\n")}
        >
          <Button variant="outline" size="sm" onPress={onRetry}>
            {usageCopy.retry}
          </Button>
        </Alert>
      ) : null}
      <UsageHistoryFigures history={history} range={range} locale={locale} />
      {history.recordedSince !== null ||
      history.unreachable.length > 0 ||
      history.notRecorded.length > 0 ? (
        <View style={styles.notes}>
          {history.recordedSince !== null ? (
            <Note text={copy.recordedSince(formatLedgerDate(history.recordedSince, locale))} />
          ) : null}
          {history.unreachable.length > 0 ? (
            <Note text={copy.unreachable(history.unreachable)} />
          ) : null}
          {history.notRecorded.length > 0 ? (
            <Note text={copy.notRecorded(history.notRecorded)} />
          ) : null}
        </View>
      ) : null}
    </View>
  );
}

function UsageHistoryFigures({
  history,
  range,
  locale,
}: {
  history: UsageHistory;
  range: HistoryRange | undefined;
  locale: string;
}) {
  const usageCopy = useUsageCopy();
  const copy = usageCopy.history;
  // Partial sums would pass for the total, so nothing shows until every host has answered.
  if (history.loading > 0) return <UsageMessage text={usageCopy.loading} />;
  if (history.answered === 0) return null;
  if (history.byModel.length === 0 && history.byDay.length === 0) {
    return <UsageMessage text={copy.empty} />;
  }
  const total = totalTokens(history.totals);
  return (
    <>
      <HistoryTiles totals={history.totals} locale={locale} />
      {range && range !== "24h" ? (
        <UsageHistoryChart days={dailyTokens(history.byDay, range, Date.now())} locale={locale} />
      ) : null}
      <LedgerTable
        title={copy.byProvider}
        rows={history.byProvider}
        total={total}
        label={(row) => ledgerProviderLabel(row.key.provider)}
        locale={locale}
        testID="usage-history-by-provider"
      />
      <LedgerTable
        title={copy.byModel}
        rows={history.byModel}
        total={total}
        label={(row) =>
          `${ledgerProviderLabel(row.key.provider)} · ${row.key.model || copy.unknownModel}`
        }
        locale={locale}
        testID="usage-history-by-model"
      />
      <LedgerTable
        title={copy.byDay}
        rows={history.byDay}
        total={null}
        label={(row) => formatLedgerDay(row.key.day ?? "", locale)}
        locale={locale}
        testID="usage-history-by-day"
      />
    </>
  );
}

/** The range's headline figures, one tile each; cache hit and cost only when there is one. */
function HistoryTiles({ totals, locale }: { totals: LedgerRow; locale: string }) {
  const copy = useUsageCopy().history;
  const count = (value: number) => formatLedgerCount(value, locale);
  const hitRate = cacheHitRate(totals);
  const cost = formatReportedCost(totals, locale);
  return (
    <View style={styles.tiles} testID="usage-history-totals">
      <Tile label={copy.tokensTitle} value={count(totalTokens(totals))} detail={null} />
      <Tile
        label={copy.callsTitle}
        value={count(totals.calls)}
        detail={totals.failedCalls > 0 ? copy.failedCalls(count(totals.failedCalls)) : null}
      />
      <Tile label={copy.requestsTitle} value={count(totals.requests)} detail={null} />
      {hitRate === null ? null : (
        <Tile label={copy.cacheHitTitle} value={formatCacheHitRate(hitRate)} detail={null} />
      )}
      {cost === null ? null : <Tile label={copy.costTitle} value={cost} detail={null} />}
    </View>
  );
}

function Tile({ label, value, detail }: { label: string; value: string; detail: string | null }) {
  return (
    <View style={[settingsStyles.card, styles.tile]}>
      <Text style={styles.tileLabel} numberOfLines={1}>
        {label}
      </Text>
      <Text style={styles.tileValue} numberOfLines={1}>
        {value}
      </Text>
      {detail ? (
        <Text style={styles.tileDetail} numberOfLines={1}>
          {detail}
        </Text>
      ) : null}
    </View>
  );
}

function LedgerTable({
  title,
  rows,
  total,
  label,
  locale,
  testID,
}: {
  title: string;
  rows: readonly LedgerRow[];
  /** The range's token total; each row's share of it draws a bar. Null draws none. */
  total: number | null;
  label: (row: LedgerRow) => string;
  locale: string;
  testID: string;
}) {
  if (rows.length === 0) return null;
  return (
    <View style={styles.table} testID={testID}>
      <Text style={settingsStyles.sectionHeaderTitle}>{title}</Text>
      <View style={settingsStyles.card}>
        {rows.map((row, index) => (
          <LedgerRowView
            key={JSON.stringify(row.key)}
            row={row}
            label={label(row)}
            share={total ? totalTokens(row) / total : null}
            locale={locale}
            bordered={index > 0}
          />
        ))}
      </View>
    </View>
  );
}

/**
 * A row's label and token total, its share of the range's tokens, then its calls, requests,
 * buckets, hit rate and cost.
 */
function LedgerRowView({
  row,
  label,
  share,
  locale,
  bordered,
}: {
  row: LedgerRow;
  label: string;
  share: number | null;
  locale: string;
  bordered: boolean;
}) {
  const copy = useUsageCopy().history;
  const tokens = (value: number) => formatLedgerCount(value, locale);
  const hitRate = cacheHitRate(row);
  const cost = formatReportedCost(row, locale);
  const details = [
    copy.calls(formatLedgerCount(row.calls, locale)),
    row.failedCalls > 0 ? copy.failedCalls(formatLedgerCount(row.failedCalls, locale)) : null,
    copy.requests(formatLedgerCount(row.requests, locale)),
    copy.input(tokens(row.inputTokens)),
    copy.output(tokens(row.outputTokens)),
    copy.cacheRead(tokens(row.cacheReadTokens)),
    copy.cacheWrite(tokens(row.cacheWriteTokens)),
    hitRate === null ? null : copy.cacheHitRate(formatCacheHitRate(hitRate)),
    cost === null ? null : copy.reportedCost(cost),
  ]
    .filter((detail) => detail !== null)
    .join(" · ");
  return (
    <View style={[styles.row, bordered && settingsStyles.rowBorder]}>
      <View style={styles.rowHeader}>
        <Text style={styles.rowLabel} numberOfLines={1}>
          {label}
        </Text>
        <Text style={styles.rowTotal}>{copy.tokens(tokens(totalTokens(row)))}</Text>
      </View>
      {share === null ? null : (
        <View style={styles.shareTrack}>
          <View
            style={[styles.shareFill, { width: `${Math.max(0, Math.min(1, share)) * 100}%` }]}
          />
        </View>
      )}
      <Text style={styles.rowDetails}>{details}</Text>
    </View>
  );
}

function Note({ text }: { text: string }) {
  return <Text style={styles.note}>{text}</Text>;
}

const styles = StyleSheet.create((theme) => ({
  body: {
    gap: theme.spacing[4],
  },
  tiles: {
    flexDirection: "row",
    flexWrap: "wrap",
    gap: theme.spacing[3],
  },
  tile: {
    flexGrow: 1,
    flexBasis: 110,
    minWidth: 0,
    gap: theme.spacing[1],
    paddingVertical: theme.spacing[3],
    paddingHorizontal: theme.spacing[4],
  },
  tileLabel: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
  tileValue: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize["2xl"],
    fontWeight: theme.fontWeight.semibold,
    fontVariant: ["tabular-nums"],
  },
  tileDetail: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
  table: {
    gap: theme.spacing[2],
  },
  row: {
    gap: theme.spacing[1.5],
    paddingVertical: theme.spacing[3],
    paddingHorizontal: theme.spacing[4],
  },
  rowHeader: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: theme.spacing[2],
  },
  rowLabel: {
    flexShrink: 1,
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
  },
  rowTotal: {
    flexShrink: 0,
    color: theme.colors.foreground,
    fontSize: theme.fontSize.sm,
    fontWeight: theme.fontWeight.medium,
    fontVariant: ["tabular-nums"],
  },
  shareTrack: {
    height: 4,
    borderRadius: 2,
    backgroundColor: theme.colors.surface3,
    overflow: "hidden",
  },
  shareFill: {
    height: "100%",
    borderRadius: 2,
    backgroundColor: theme.colors.accent,
  },
  rowDetails: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    lineHeight: theme.fontSize.sm * 1.4,
  },
  notes: {
    gap: theme.spacing[1],
  },
  note: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    lineHeight: theme.fontSize.sm * 1.4,
  },
}));

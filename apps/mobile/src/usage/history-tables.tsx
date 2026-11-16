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
import {
  cacheHitRate,
  ledgerProviderLabel,
  totalTokens,
  type HostHistoryTotals,
  type LedgerRow,
  type UsageHistory,
} from "./history-model";
import { UsageMessage } from "./usage-section";

/** History's tables for the hosts that answered, and notes on those that did not. */
export function UsageHistoryBody({
  history,
  onRetry,
}: {
  history: UsageHistory;
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
      <UsageHistoryTables history={history} locale={locale} />
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
    </View>
  );
}

function UsageHistoryTables({ history, locale }: { history: UsageHistory; locale: string }) {
  const usageCopy = useUsageCopy();
  const copy = usageCopy.history;
  // Partial sums would pass for the total, so nothing shows until every host has answered.
  if (history.loading > 0) return <UsageMessage text={usageCopy.loading} />;
  if (history.answered === 0) return null;
  if (history.byModel.length === 0 && history.byDay.length === 0) {
    return <UsageMessage text={copy.empty} />;
  }
  return (
    <>
      <LedgerTable
        title={copy.byProvider}
        rows={history.byProvider}
        label={(row) => ledgerProviderLabel(row.key.provider)}
        locale={locale}
        testID="usage-history-by-provider"
      />
      <LedgerTable
        title={copy.byModel}
        rows={history.byModel}
        label={(row) =>
          `${ledgerProviderLabel(row.key.provider)} · ${row.key.model || copy.unknownModel}`
        }
        locale={locale}
        testID="usage-history-by-model"
      />
      <LedgerTable
        title={copy.byDay}
        rows={history.byDay}
        label={(row) => formatLedgerDay(row.key.day ?? "", locale)}
        locale={locale}
        testID="usage-history-by-day"
      />
    </>
  );
}

function LedgerTable({
  title,
  rows,
  label,
  locale,
  testID,
}: {
  title: string;
  rows: readonly LedgerRow[];
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
            locale={locale}
            bordered={index > 0}
          />
        ))}
      </View>
    </View>
  );
}

/** A row's label and token total, then its calls, requests, buckets, hit rate and cost. */
function LedgerRowView({
  row,
  label,
  locale,
  bordered,
}: {
  row: LedgerRow;
  label: string;
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
      <Text style={styles.rowDetails}>{details}</Text>
    </View>
  );
}

/** A host's totals: calls, requests and tokens on one line, with when its history starts. */
export function HostHistoryTotalsBody({
  totals,
  hostLabel,
  onRetry,
}: {
  totals: Exclude<HostHistoryTotals, { kind: "hidden" }>;
  hostLabel: string;
  onRetry: () => void;
}) {
  const usageCopy = useUsageCopy();
  const copy = usageCopy.history;
  const { i18n } = useTranslation();
  const locale = i18n.language;
  switch (totals.kind) {
    case "loading":
      return <UsageMessage text={usageCopy.loading} />;
    case "notRecorded":
      return <UsageMessage text={copy.notRecorded([hostLabel])} />;
    case "error":
      return (
        <Alert variant="error" title={usageCopy.errorTitle} description={totals.message}>
          <Button variant="outline" size="sm" onPress={onRetry}>
            {usageCopy.retry}
          </Button>
        </Alert>
      );
    case "ready": {
      const row = totals.totals;
      const summary = [
        copy.calls(formatLedgerCount(row.calls, locale)),
        copy.requests(formatLedgerCount(row.requests, locale)),
        copy.tokens(formatLedgerCount(totalTokens(row), locale)),
      ].join(" · ");
      return (
        <View style={styles.body}>
          <View style={[settingsStyles.card, styles.row]}>
            <Text style={styles.rowLabel}>{summary}</Text>
          </View>
          {totals.recordedSince !== null ? (
            <View style={styles.notes}>
              <Note text={copy.recordedSince(formatLedgerDate(totals.recordedSince, locale))} />
            </View>
          ) : null}
        </View>
      );
    }
  }
}

function Note({ text }: { text: string }) {
  return <Text style={styles.note}>{text}</Text>;
}

const styles = StyleSheet.create((theme) => ({
  body: {
    gap: theme.spacing[4],
  },
  table: {
    gap: theme.spacing[2],
  },
  row: {
    gap: theme.spacing[1],
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

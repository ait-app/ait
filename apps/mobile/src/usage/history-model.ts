import type { ProviderUsageLedgerResponseMessage } from "@ait/protocol/messages";
import { AGENT_PROVIDER_DEFINITIONS } from "@ait/protocol/provider-manifest";

/** One host's ledger summary for a range, as `provider.usage.ledger` answers it. */
export type LedgerSummary = ProviderUsageLedgerResponseMessage["payload"];
export type LedgerRow = LedgerSummary["rows"][number];
export type LedgerKey = LedgerRow["key"];

/** The History ranges, each ending now. */
export const HISTORY_RANGES = ["24h", "7d", "30d"] as const;
export type HistoryRange = (typeof HISTORY_RANGES)[number];

const DAY_MS = 24 * 60 * 60 * 1000;
const RANGE_MS: Record<HistoryRange, number> = {
  "24h": DAY_MS,
  "7d": 7 * DAY_MS,
  "30d": 30 * DAY_MS,
};

/** The range `[now - range, now)` in Unix ms, as the ledger request takes it. */
export function historyWindow(range: HistoryRange, now: number): { from: number; to: number } {
  return { from: now - RANGE_MS[range], to: now };
}

/** Whether any call behind a row reported partial token usage or none, so its totals are minimums. */
export function isLowerBound(row: LedgerRow): boolean {
  return row.lowerBoundCalls > 0 || row.missingCalls > 0;
}

/** Every token the row counts. The buckets are disjoint, so their sum counts nothing twice. */
export function totalTokens(row: LedgerRow): number {
  return row.inputTokens + row.outputTokens + row.cacheReadTokens + row.cacheWriteTokens;
}

/** Cache read over all prompt input, or null when no prompt input was counted. */
export function cacheHitRate(row: LedgerRow): number | null {
  const denominator = row.inputTokens + row.cacheReadTokens;
  return denominator > 0 ? row.cacheReadTokens / denominator : null;
}

/** A row's key as one string, for summing rows of the same dimensions across hosts. */
function keyId(key: LedgerKey): string {
  return JSON.stringify([key.provider, key.model, key.source, key.outcome, key.day]);
}

function addRows(a: LedgerRow, b: LedgerRow): LedgerRow {
  return {
    key: a.key,
    calls: a.calls + b.calls,
    failedCalls: a.failedCalls + b.failedCalls,
    requests: a.requests + b.requests,
    inputTokens: a.inputTokens + b.inputTokens,
    outputTokens: a.outputTokens + b.outputTokens,
    cacheReadTokens: a.cacheReadTokens + b.cacheReadTokens,
    cacheWriteTokens: a.cacheWriteTokens + b.cacheWriteTokens,
    reportedCostUsd: a.reportedCostUsd + b.reportedCostUsd,
    costReportedRequests: a.costReportedRequests + b.costReportedRequests,
    lowerBoundCalls: a.lowerBoundCalls + b.lowerBoundCalls,
    missingCalls: a.missingCalls + b.missingCalls,
  };
}

/**
 * Rows summed by the key `rekey` gives each, in first-seen order. Summing the same key from every
 * host merges hosts; rekeying to fewer dimensions rolls rows up (models into their provider).
 */
export function sumRows(
  rows: readonly LedgerRow[],
  rekey: (key: LedgerKey) => LedgerKey = (key) => key,
): LedgerRow[] {
  const sums = new Map<string, LedgerRow>();
  for (const row of rows) {
    const key = rekey(row.key);
    const id = keyId(key);
    const current = sums.get(id);
    sums.set(id, current ? addRows(current, row) : { ...row, key });
  }
  return [...sums.values()];
}

const EMPTY_ROW: LedgerRow = {
  key: {},
  calls: 0,
  failedCalls: 0,
  requests: 0,
  inputTokens: 0,
  outputTokens: 0,
  cacheReadTokens: 0,
  cacheWriteTokens: 0,
  reportedCostUsd: 0,
  costReportedRequests: 0,
  lowerBoundCalls: 0,
  missingCalls: 0,
};

/** Every row summed into one, for a host's totals. */
export function sumAll(rows: readonly LedgerRow[]): LedgerRow {
  return sumRows(rows, () => ({}))[0] ?? EMPTY_ROW;
}

/** Heaviest first by tokens, then by calls; ties keep their order. */
export function byUsage(a: LedgerRow, b: LedgerRow): number {
  return totalTokens(b) - totalTokens(a) || b.calls - a.calls;
}

/** Newest day first. */
export function byDayDescending(a: LedgerRow, b: LedgerRow): number {
  return (b.key.day ?? "").localeCompare(a.key.day ?? "");
}

/** Whether a ledger request failed because the host keeps no ledger, though it lists the method. */
export function isLedgerUnsupported(error: unknown): boolean {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    error.code === "unsupported_capability"
  );
}

/** A summary with the start of the range it was asked for, which the client picks at fetch time. */
export type LedgerAnswer = LedgerSummary & { from: number };

export interface LedgerQueryState {
  data: LedgerAnswer | undefined;
  error: unknown;
  isFetching: boolean;
}

/** One host's part of the History view: its two summaries for the chosen range. */
export interface LedgerHostInput {
  serverId: string;
  label: string;
  isConnected: boolean;
  /** From the host's advertised methods; null until it has connected and said. */
  supportsLedger: boolean | null;
  byModel: LedgerQueryState;
  byDay: LedgerQueryState;
}

export interface LedgerHostFailure {
  label: string;
  message: string;
}

/**
 * The History view over one host or several. Hosts that could not answer are listed, never counted
 * as zero; hosts without a ledger are left out, or listed as not recording when they claimed one.
 */
export interface UsageHistory {
  /** Rows by provider and model, summed over the hosts that answered. */
  byModel: LedgerRow[];
  /** `byModel` rolled up by provider. */
  byProvider: LedgerRow[];
  /** Rows by UTC day, newest first. */
  byDay: LedgerRow[];
  /**
   * When the oldest answering host started recording, if that is inside the range: nothing before
   * it was recorded anywhere. Null when recording covers the whole range or no host answered.
   */
  recordedSince: number | null;
  /** How many hosts answered. */
  answered: number;
  /** Hosts still loading with nothing to show yet. */
  loading: number;
  unreachable: string[];
  failed: LedgerHostFailure[];
  notRecorded: string[];
  isRefreshing: boolean;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** A failed summary request, unless the host has its data from before. */
function failure(query: LedgerQueryState): unknown {
  return query.data ? null : query.error || null;
}

/** Combine each host's summaries into the History view. */
export function combineUsageHistory(hosts: readonly LedgerHostInput[]): UsageHistory {
  const history: UsageHistory = {
    byModel: [],
    byProvider: [],
    byDay: [],
    recordedSince: null,
    answered: 0,
    loading: 0,
    unreachable: [],
    failed: [],
    notRecorded: [],
    isRefreshing: false,
  };
  const modelRows: LedgerRow[] = [];
  const dayRows: LedgerRow[] = [];
  let since = Infinity;
  let from = Infinity;
  for (const host of hosts) {
    // An offline host is listed unless it is known to keep no ledger; it may hold history.
    if (!host.isConnected) {
      if (host.supportsLedger !== false) history.unreachable.push(host.label);
      continue;
    }
    if (host.supportsLedger !== true) continue;
    const error = failure(host.byModel) ?? failure(host.byDay);
    if (error) {
      if (isLedgerUnsupported(error)) history.notRecorded.push(host.label);
      else history.failed.push({ label: host.label, message: errorMessage(error) });
      continue;
    }
    const byModel = host.byModel.data;
    const byDay = host.byDay.data;
    if (!byModel || !byDay) {
      history.loading += 1;
      continue;
    }
    history.answered += 1;
    if (host.byModel.isFetching || host.byDay.isFetching) history.isRefreshing = true;
    modelRows.push(...byModel.rows);
    dayRows.push(...byDay.rows);
    since = Math.min(since, byModel.since);
    from = Math.min(from, byModel.from);
  }
  if (since > from && Number.isFinite(since)) history.recordedSince = since;
  history.byModel = sumRows(modelRows).sort(byUsage);
  history.byProvider = sumRows(modelRows, (key) => ({ provider: key.provider })).sort(byUsage);
  history.byDay = sumRows(dayRows).sort(byDayDescending);
  return history;
}

/** Whether any row of the view is only a lower bound, so the view explains the "≥" marker. */
export function hasLowerBound(history: Pick<UsageHistory, "byModel" | "byDay">): boolean {
  return history.byModel.some(isLowerBound) || history.byDay.some(isLowerBound);
}

/** What a host's settings page shows of its ledger. */
export type HostHistoryTotals =
  | { kind: "hidden" }
  | { kind: "loading" }
  | { kind: "notRecorded" }
  | { kind: "error"; message: string }
  | { kind: "ready"; totals: LedgerRow; recordedSince: number | null };

/**
 * A host's totals over its by-model summary. Hidden while the host is offline or does not offer
 * the ledger; a host that offers it but keeps none says so instead of failing.
 */
export function resolveHostHistoryTotals(input: {
  isConnected: boolean;
  supportsLedger: boolean | null;
  query: LedgerQueryState;
}): HostHistoryTotals {
  const { isConnected, supportsLedger, query } = input;
  if (!isConnected || supportsLedger !== true) return { kind: "hidden" };
  const error = failure(query);
  if (error) {
    return isLedgerUnsupported(error)
      ? { kind: "notRecorded" }
      : { kind: "error", message: errorMessage(error) };
  }
  if (!query.data) return { kind: "loading" };
  const { rows, since, from } = query.data;
  return { kind: "ready", totals: sumAll(rows), recordedSince: since > from ? since : null };
}

/** A provider ID as the app names it: "claude" reads "Claude"; an unknown ID reads as itself. */
export function ledgerProviderLabel(provider: string | undefined): string {
  if (!provider) return "";
  return (
    AGENT_PROVIDER_DEFINITIONS.find((definition) => definition.id === provider)?.label ?? provider
  );
}

import { describe, expect, it } from "vitest";
import {
  cacheHitRate,
  combineUsageHistory,
  hasLowerBound,
  historyWindow,
  isLedgerUnsupported,
  isLowerBound,
  ledgerProviderLabel,
  resolveHostHistoryTotals,
  sumAll,
  sumRows,
  totalTokens,
  type LedgerAnswer,
  type LedgerHostInput,
  type LedgerKey,
  type LedgerQueryState,
  type LedgerRow,
} from "./history-model";

const FROM = Date.parse("2026-11-04T00:00:00Z");

function row(key: LedgerKey, figures: Partial<Omit<LedgerRow, "key">> = {}): LedgerRow {
  return {
    key,
    calls: 1,
    failedCalls: 0,
    requests: 2,
    inputTokens: 100,
    outputTokens: 50,
    cacheReadTokens: 300,
    cacheWriteTokens: 10,
    reportedCostUsd: 0,
    costReportedRequests: 0,
    lowerBoundCalls: 0,
    missingCalls: 0,
    ...figures,
  };
}

function answer(rows: LedgerRow[], since = FROM - 1): LedgerAnswer {
  return { requestId: "r", rows, since, from: FROM };
}

function ready(data: LedgerAnswer): LedgerQueryState {
  return { data, error: null, isFetching: false };
}

function failed(error: unknown): LedgerQueryState {
  return { data: undefined, error, isFetching: false };
}

const pending: LedgerQueryState = { data: undefined, error: null, isFetching: true };

function rpcError(code: string): Error {
  return Object.assign(new Error(`ledger failed code=${code}`), { code });
}

function host(
  label: string,
  input: Partial<Omit<LedgerHostInput, "label" | "serverId">> = {},
): LedgerHostInput {
  return {
    serverId: label.toLowerCase(),
    label,
    isConnected: true,
    supportsLedger: true,
    byModel: ready(answer([])),
    byDay: ready(answer([])),
    ...input,
  };
}

describe("historyWindow", () => {
  it("ends now and starts one range earlier", () => {
    const now = Date.parse("2026-11-11T12:00:00Z");
    expect(historyWindow("24h", now)).toEqual({ from: now - 86_400_000, to: now });
    expect(historyWindow("7d", now).from).toBe(Date.parse("2026-11-04T12:00:00Z"));
    expect(historyWindow("30d", now).from).toBe(Date.parse("2026-10-12T12:00:00Z"));
  });
});

describe("row figures", () => {
  it("totals the disjoint buckets once each", () => {
    expect(totalTokens(row({}))).toBe(460);
  });

  it("rates cache hits over all prompt input, and only when there was some", () => {
    expect(cacheHitRate(row({}, { inputTokens: 100, cacheReadTokens: 300 }))).toBe(0.75);
    expect(cacheHitRate(row({}, { inputTokens: 0, cacheReadTokens: 0 }))).toBeNull();
    expect(cacheHitRate(row({}, { inputTokens: 40, cacheReadTokens: 0 }))).toBe(0);
  });

  it("marks rows with partial or missing calls as lower bounds", () => {
    expect(isLowerBound(row({}))).toBe(false);
    expect(isLowerBound(row({}, { lowerBoundCalls: 1 }))).toBe(true);
    expect(isLowerBound(row({}, { missingCalls: 2 }))).toBe(true);
    expect(hasLowerBound({ byModel: [row({})], byDay: [row({}, { missingCalls: 1 })] })).toBe(true);
    expect(hasLowerBound({ byModel: [row({})], byDay: [row({})] })).toBe(false);
  });
});

describe("sumRows", () => {
  it("sums every figure of rows with the same key and keeps other keys apart", () => {
    const sums = sumRows([
      row({ provider: "claude", model: "opus" }, { reportedCostUsd: 0.5, costReportedRequests: 1 }),
      row({ provider: "codex", model: "gpt" }),
      row(
        { provider: "claude", model: "opus" },
        { calls: 2, failedCalls: 1, lowerBoundCalls: 1, missingCalls: 1 },
      ),
    ]);
    expect(sums).toEqual([
      row(
        { provider: "claude", model: "opus" },
        {
          calls: 3,
          failedCalls: 1,
          requests: 4,
          inputTokens: 200,
          outputTokens: 100,
          cacheReadTokens: 600,
          cacheWriteTokens: 20,
          reportedCostUsd: 0.5,
          costReportedRequests: 1,
          lowerBoundCalls: 1,
          missingCalls: 1,
        },
      ),
      row({ provider: "codex", model: "gpt" }),
    ]);
  });

  it("rolls models up into their provider when rekeyed", () => {
    const sums = sumRows(
      [
        row({ provider: "claude", model: "opus" }),
        row({ provider: "claude", model: "sonnet" }),
        row({ provider: "codex", model: "gpt" }),
      ],
      (key) => ({ provider: key.provider }),
    );
    expect(sums.map((sum) => [sum.key, sum.calls])).toEqual([
      [{ provider: "claude" }, 2],
      [{ provider: "codex" }, 1],
    ]);
  });

  it("sums everything into one total, or zeros without rows", () => {
    expect(sumAll([row({ day: "2026-11-10" }), row({ day: "2026-11-11" })])).toMatchObject({
      key: {},
      calls: 2,
      requests: 4,
    });
    expect(sumAll([])).toMatchObject({ calls: 0, inputTokens: 0, missingCalls: 0 });
  });
});

describe("combineUsageHistory", () => {
  it("sums the hosts that answered by key and orders the tables", () => {
    const history = combineUsageHistory([
      host("Laptop", {
        byModel: ready(answer([row({ provider: "codex", model: "gpt" })])),
        byDay: ready(answer([row({ day: "2026-11-10" })])),
      }),
      host("Server", {
        byModel: ready(
          answer([
            row({ provider: "codex", model: "gpt" }),
            row({ provider: "claude", model: "opus" }, { inputTokens: 10_000 }),
            row({ provider: "claude", model: "sonnet" }),
          ]),
        ),
        byDay: ready(answer([row({ day: "2026-11-10" }), row({ day: "2026-11-11" })])),
      }),
    ]);
    expect(history.answered).toBe(2);
    expect(history.byModel.map((sum) => [sum.key.model, sum.calls])).toEqual([
      ["opus", 1],
      ["gpt", 2],
      ["sonnet", 1],
    ]);
    expect(history.byProvider.map((sum) => [sum.key, sum.calls])).toEqual([
      [{ provider: "claude" }, 2],
      [{ provider: "codex" }, 2],
    ]);
    expect(history.byDay.map((sum) => [sum.key.day, sum.calls])).toEqual([
      ["2026-11-11", 1],
      ["2026-11-10", 2],
    ]);
    expect(history.unreachable).toEqual([]);
    expect(history.failed).toEqual([]);
  });

  it("lists unreachable and failed hosts instead of counting them as zero", () => {
    const history = combineUsageHistory([
      host("Laptop", { byModel: ready(answer([row({ provider: "codex", model: "gpt" })])) }),
      host("Offline", { isConnected: false, byModel: pending, byDay: pending }),
      host("Broken", { byModel: failed(rpcError("agent_io")) }),
    ]);
    expect(history.answered).toBe(1);
    expect(history.byModel).toHaveLength(1);
    expect(history.unreachable).toEqual(["Offline"]);
    expect(history.failed).toEqual([{ label: "Broken", message: "ledger failed code=agent_io" }]);
  });

  it("reports a host whose ledger did not open as not recording, not as a failure", () => {
    const history = combineUsageHistory([
      host("Laptop"),
      host("NoLedger", { byDay: failed(rpcError("unsupported_capability")) }),
    ]);
    expect(history.notRecorded).toEqual(["NoLedger"]);
    expect(history.failed).toEqual([]);
    expect(history.answered).toBe(1);
  });

  it("leaves out hosts known to lack the ledger, and connected hosts that have not said", () => {
    const history = combineUsageHistory([
      host("Old", { supportsLedger: false }),
      host("OldOffline", { supportsLedger: false, isConnected: false }),
      host("Handshaking", { supportsLedger: null }),
      host("NeverSeen", { supportsLedger: null, isConnected: false }),
    ]);
    expect(history.answered).toBe(0);
    expect(history.loading).toBe(0);
    expect(history.unreachable).toEqual(["NeverSeen"]);
    expect(history.notRecorded).toEqual([]);
  });

  it("counts hosts still loading and keeps data a failed refresh could not replace", () => {
    const stale: LedgerQueryState = {
      data: answer([row({ provider: "codex", model: "gpt" })]),
      error: rpcError("agent_io"),
      isFetching: false,
    };
    const history = combineUsageHistory([
      host("Laptop", { byModel: stale }),
      host("Server", { byDay: pending }),
      host("Refreshing", { byModel: { ...ready(answer([])), isFetching: true } }),
    ]);
    expect(history.answered).toBe(2);
    expect(history.loading).toBe(1);
    expect(history.failed).toEqual([]);
    expect(history.isRefreshing).toBe(true);
  });

  it("notes when the oldest host's history starts inside the range", () => {
    const since = (value: number) => ({
      byModel: ready(answer([], value)),
      byDay: ready(answer([], value)),
    });
    const late = FROM + 3_600_000;
    expect(
      combineUsageHistory([host("A", since(late)), host("B", since(late + 1))]).recordedSince,
    ).toBe(late);
    expect(
      combineUsageHistory([host("A", since(late)), host("B", since(FROM - 1))]).recordedSince,
    ).toBeNull();
    expect(combineUsageHistory([host("A", { isConnected: false })]).recordedSince).toBeNull();
  });
});

describe("resolveHostHistoryTotals", () => {
  const base = { isConnected: true, supportsLedger: true };

  it("hides the totals for offline hosts and hosts without the ledger", () => {
    const query = ready(answer([]));
    expect(resolveHostHistoryTotals({ ...base, isConnected: false, query })).toEqual({
      kind: "hidden",
    });
    expect(resolveHostHistoryTotals({ ...base, supportsLedger: false, query })).toEqual({
      kind: "hidden",
    });
    expect(resolveHostHistoryTotals({ ...base, supportsLedger: null, query })).toEqual({
      kind: "hidden",
    });
  });

  it("says the host is not recording when its ledger did not open", () => {
    expect(
      resolveHostHistoryTotals({ ...base, query: failed(rpcError("unsupported_capability")) }),
    ).toEqual({ kind: "notRecorded" });
    expect(resolveHostHistoryTotals({ ...base, query: failed(new Error("disk")) })).toEqual({
      kind: "error",
      message: "disk",
    });
    expect(resolveHostHistoryTotals({ ...base, query: pending })).toEqual({ kind: "loading" });
  });

  it("sums the host's rows and notes a start inside the range", () => {
    const totals = resolveHostHistoryTotals({
      ...base,
      query: ready(
        answer(
          [row({ provider: "codex", model: "gpt" }), row({ provider: "claude", model: "opus" })],
          FROM + 1,
        ),
      ),
    });
    expect(totals).toMatchObject({
      kind: "ready",
      totals: { calls: 2, requests: 4 },
      recordedSince: FROM + 1,
    });
  });
});

describe("isLedgerUnsupported", () => {
  it("recognizes only the unsupported capability code", () => {
    expect(isLedgerUnsupported(rpcError("unsupported_capability"))).toBe(true);
    expect(isLedgerUnsupported(rpcError("agent_io"))).toBe(false);
    expect(isLedgerUnsupported(new Error("unsupported_capability"))).toBe(false);
    expect(isLedgerUnsupported(null)).toBe(false);
  });
});

describe("ledgerProviderLabel", () => {
  it("names known providers and keeps unknown IDs", () => {
    expect(ledgerProviderLabel("claude")).toBe("Claude");
    expect(ledgerProviderLabel("acme")).toBe("acme");
    expect(ledgerProviderLabel(undefined)).toBe("");
  });
});

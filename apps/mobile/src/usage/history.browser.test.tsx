import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { UsageHistoryBody } from "./history-tables";
import { combineUsageHistory, type LedgerRow, type UsageHistory } from "./history-model";

vi.mock("@/constants/platform", () => ({ isNative: false, isWeb: true }));
vi.mock("@/constants/layout", () => ({
  useIsCompactFormFactor: () => false,
  getIsElectronRuntimeMac: () => false,
  WORKSPACE_SECONDARY_HEADER_HEIGHT: 36,
}));
// The usage section's title-row controls pull in the native host runtime.
vi.mock("./usage-section", () => ({
  UsageMessage: ({ text }: { text: string }) => <span>{text}</span>,
}));

const FROM = Date.parse("2026-11-04T00:00:00Z");
let root: Root | null = null;
let container: HTMLDivElement | null = null;
const retry = vi.fn();

function row(key: LedgerRow["key"], figures: Partial<LedgerRow> = {}): LedgerRow {
  return {
    key,
    calls: 2,
    failedCalls: 0,
    requests: 3,
    inputTokens: 1_000,
    outputTokens: 500,
    cacheReadTokens: 3_000,
    cacheWriteTokens: 0,
    reportedCostUsd: 0,
    costReportedRequests: 0,
    lowerBoundCalls: 0,
    missingCalls: 0,
    ...figures,
  };
}

function ready(rows: LedgerRow[]) {
  return {
    data: { requestId: "r", rows, since: FROM - 1, from: FROM },
    error: null,
    isFetching: false,
  };
}

function render(history: UsageHistory) {
  if (!container) {
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  }
  act(() => root?.render(<UsageHistoryBody history={history} onRetry={retry} />));
  return container;
}

beforeEach(async () => {
  await i18n.changeLanguage("en");
  vi.stubGlobal("React", React);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
});

afterEach(async () => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
  vi.clearAllMocks();
  vi.unstubAllGlobals();
  await i18n.changeLanguage("en");
});

describe("usage history in Chromium", () => {
  it("marks lower bounds, lists unreachable hosts, and does not treat a missing ledger as an error", () => {
    const node = render(
      combineUsageHistory([
        {
          serverId: "laptop",
          label: "Laptop",
          isConnected: true,
          supportsLedger: true,
          byModel: ready([
            row({ provider: "claude", model: "opus" }, { lowerBoundCalls: 1 }),
            row(
              { provider: "codex", model: "gpt" },
              { reportedCostUsd: 0.25, costReportedRequests: 3 },
            ),
          ]),
          byDay: ready([row({ day: "2026-11-10" })]),
        },
        {
          serverId: "offline",
          label: "Offline",
          isConnected: false,
          supportsLedger: true,
          byModel: ready([]),
          byDay: ready([]),
        },
        {
          serverId: "no-ledger",
          label: "NoLedger",
          isConnected: true,
          supportsLedger: true,
          byModel: {
            data: undefined,
            error: Object.assign(new Error("x"), { code: "unsupported_capability" }),
            isFetching: false,
          },
          byDay: ready([]),
        },
      ]),
    );
    const byModel = node.querySelector('[data-testid="usage-history-by-model"]')!;
    expect(byModel.textContent).toContain("Claude · opus");
    expect(byModel.textContent).toContain("Tokens ≥4.5K");
    expect(byModel.textContent).toContain("Cache hit 75%");
    expect(byModel.textContent).toContain("Provider-reported cost $0.25");
    expect(node.querySelector('[data-testid="usage-history-by-provider"]')?.textContent).toContain(
      "Codex",
    );
    expect(node.querySelector('[data-testid="usage-history-by-day"]')?.textContent).toContain(
      "Nov 10",
    );
    expect(node.textContent).toContain("≥ marks a lower bound");
    expect(node.textContent).toContain("Not counted, unable to reach: Offline");
    expect(node.textContent).toContain("Usage history is not recorded on NoLedger");
    expect(node.textContent).not.toContain("Unable to load");
  });

  it("waits for every host before showing sums, and offers a retry for failed hosts", () => {
    const loading = render(
      combineUsageHistory([
        {
          serverId: "laptop",
          label: "Laptop",
          isConnected: true,
          supportsLedger: true,
          byModel: ready([row({ provider: "codex", model: "gpt" })]),
          byDay: ready([]),
        },
        {
          serverId: "server",
          label: "Server",
          isConnected: true,
          supportsLedger: true,
          byModel: { data: undefined, error: null, isFetching: true },
          byDay: { data: undefined, error: null, isFetching: true },
        },
      ]),
    );
    expect(loading.textContent).toContain("Loading usage...");
    expect(loading.querySelector('[data-testid="usage-history-by-model"]')).toBeNull();

    const failed = render(
      combineUsageHistory([
        {
          serverId: "server",
          label: "Server",
          isConnected: true,
          supportsLedger: true,
          byModel: { data: undefined, error: new Error("disk full"), isFetching: false },
          byDay: ready([]),
        },
      ]),
    );
    expect(failed.textContent).toContain("Unable to load usage history from Server: disk full");
    act(() =>
      [...failed.querySelectorAll<HTMLElement>('[role="button"]')]
        .find((button) => button.textContent === "Try again")!
        .click(),
    );
    expect(retry).toHaveBeenCalledOnce();
  });
});

import { afterEach, describe, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import {
  formatAmount,
  formatCacheHitRate,
  formatDisplayPct,
  formatLedgerCount,
  formatLedgerDate,
  formatLedgerDay,
  formatLedgerTokens,
  formatReportedCost,
  formatResetLabel,
  formatRunOutLabel,
} from "./format";

afterEach(async () => {
  vi.useRealTimers();
  await i18n.changeLanguage("en");
});

it("localizes remaining percentages, reset times and exhaustion times", async () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-10-10T12:00:00Z"));
  await i18n.changeLanguage("zh-CN");
  expect(formatDisplayPct(75, "remaining")).toBe("剩余 75%");
  expect(formatDisplayPct(25, "used")).toBe("25%");
  expect(formatResetLabel("2026-10-10T14:00:00Z")).toBe("2 小时后重置");
  expect(formatRunOutLabel("2026-10-10T12:30:00Z")).toBe("30 分钟后耗尽");
  expect(formatResetLabel("2026-10-10T11:00:00Z")).toBe("正在重置");
  expect(formatResetLabel("invalid")).toBeNull();
  expect(formatRunOutLabel(null)).toBeNull();
});

describe("formatAmount", () => {
  it("formats yuan with its actual currency, including zero and debt", () => {
    expect(formatAmount(12.01, "cny", "en")).toBe("CN¥12.01");
    expect(formatAmount(0, "cny", "zh-CN")).toBe("¥0.00");
    expect(formatAmount(-0.01, "cny", "zh-CN")).toBe("-¥0.01");
  });
  it("groups thousands in the app's language", () => {
    expect(formatAmount(12345, "credits", "en")).toBe("12,345");
    expect(formatAmount(12345, "requests", "en")).toBe("12,345");
    expect(formatAmount(12345, "credits", "fr").replace(/\s/g, " ")).toBe("12 345");
  });

  it("formats dollars as the language writes currency", () => {
    expect(formatAmount(1234.5, "usd", "en")).toBe("$1,234.50");
    expect(formatAmount(1234.5, "usd", "fr").replace(/\s/g, " ")).toBe("1 234,50 $US");
  });
});

it("retained reset times that have passed do not claim the quota is resetting", () => {
  const past = "2000-01-01T00:00:00.000Z";
  expect(formatResetLabel(past, true)).toBe("Reset time passed");
  expect(formatResetLabel(past)).toBe("resetting now");
  expect(formatResetLabel(null, true)).toBeNull();
});

describe("ledger figures", () => {
  it("compacts counts past a thousand in the app's language", () => {
    expect(formatLedgerCount(980, "en")).toBe("980");
    expect(formatLedgerCount(12_345, "en")).toBe("12.3K");
    expect(formatLedgerCount(1_250_000, "en")).toBe("1.3M");
  });

  it("marks lower-bound token figures with ≥", () => {
    expect(formatLedgerTokens(12_345, false, "en")).toBe("12.3K");
    expect(formatLedgerTokens(12_345, true, "en")).toBe("≥12.3K");
    expect(formatLedgerTokens(0, true, "en")).toBe("≥0");
  });

  it("writes cache hit rates as whole percents", () => {
    expect(formatCacheHitRate(0.756)).toBe("76%");
    expect(formatCacheHitRate(0)).toBe("0%");
  });

  it("shows reported cost only when a provider reported one, and marks partial reports", () => {
    const row = { reportedCostUsd: 1.5, costReportedRequests: 2, requests: 2 };
    expect(formatReportedCost(row, "en")).toBe("$1.50");
    expect(formatReportedCost({ ...row, costReportedRequests: 1 }, "en")).toBe("≥$1.50");
    expect(formatReportedCost({ ...row, costReportedRequests: 0 }, "en")).toBeNull();
    expect(formatReportedCost({ ...row, reportedCostUsd: 0.0042 }, "en")).toBe("$0.0042");
  });

  it("writes UTC ledger days and history starts in the app's language", () => {
    expect(formatLedgerDay("2026-11-10", "en")).toBe("Nov 10");
    expect(formatLedgerDay("2026-11-10", "zh-CN")).toBe("11月10日");
    expect(formatLedgerDay("someday", "en")).toBe("someday");
    expect(formatLedgerDate(Date.parse("2026-11-02T12:00:00Z"), "en")).toBe("Nov 2, 2026");
  });
});

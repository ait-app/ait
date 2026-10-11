import { afterEach, describe, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { formatAmount, formatDisplayPct, formatResetLabel, formatRunOutLabel } from "./format";

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

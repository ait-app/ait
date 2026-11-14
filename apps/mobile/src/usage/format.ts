import { formatTokenCount } from "@/components/context-window-meter.utils";
import type { TFunction } from "i18next";
import { i18n } from "@/i18n/i18next";
import type { UsageDisplayAs } from "./preferences";
import type { UsageBalanceUnit } from "./types";

export function clampPct(value: number): number {
  return Math.max(0, Math.min(100, value));
}

export function formatPct(value: number): string {
  return `${Math.round(clampPct(value))}%`;
}

/** "31%" of the window used, or "69% left" of it. */
export function formatDisplayPct(
  value: number,
  displayAs: UsageDisplayAs,
  t: TFunction = i18n.t.bind(i18n),
): string {
  return displayAs === "used"
    ? formatPct(value)
    : t("providerUsage.remaining", { amount: formatPct(value) });
}

/** Time until `iso` in the largest whole unit, "" once it has passed, or null if unparseable. */
function relativeDuration(iso: string, t: TFunction): string | null {
  const diffMs = new Date(iso).getTime() - Date.now();
  if (!Number.isFinite(diffMs)) return null;
  if (diffMs <= 0) return "";
  const diffMinutes = Math.floor(diffMs / 60_000);
  const diffHours = Math.floor(diffMinutes / 60);
  const diffDays = Math.floor(diffHours / 24);
  if (diffDays > 0) return t("providerUsage.days", { count: diffDays });
  if (diffHours > 0) return t("providerUsage.hours", { count: diffHours });
  return t("providerUsage.minutes", { count: diffMinutes });
}

export function formatResetLabel(
  iso: string | null | undefined,
  t: TFunction = i18n.t.bind(i18n),
  retained = false,
): string | null {
  if (!iso) return null;
  if (retained && Date.parse(iso) <= Date.now()) return t("providerUsage.resetTimePassed");
  const time = relativeDuration(iso, t);
  if (time === null) return null;
  return time === "" ? t("providerUsage.resettingNow") : t("providerUsage.resets", { time });
}

export function formatRunOutLabel(
  iso: string | null | undefined,
  t: TFunction = i18n.t.bind(i18n),
): string | null {
  if (!iso) return null;
  const time = relativeDuration(iso, t);
  if (time === null) return null;
  return time === "" ? t("providerUsage.runsOutNow") : t("providerUsage.runsOut", { time });
}

/** A balance amount as the app's language writes it: "$1,234.50", "12,345". */
export function formatAmount(value: number, unit: UsageBalanceUnit, locale: string): string {
  switch (unit) {
    case "usd":
    case "cny":
      return new Intl.NumberFormat(locale, {
        style: "currency",
        currency: unit === "cny" ? "CNY" : "USD",
      }).format(value);
    case "tokens":
      return formatTokenCount(value);
    default:
      return new Intl.NumberFormat(locale, { maximumFractionDigits: 2 }).format(value);
  }
}

const LEDGER_UNITS = [
  { size: 1_000_000_000, suffix: "B" },
  { size: 1_000_000, suffix: "M" },
  { size: 1_000, suffix: "K" },
] as const;

/**
 * A ledger count compacted past a thousand with K, M and B in every language, never 万 or 亿:
 * "980", "12.3K", "1.3M", "4.6B". Only the decimal separator follows the app's language.
 */
export function formatLedgerCount(value: number, locale: string): string {
  const digits = new Intl.NumberFormat(locale, { maximumFractionDigits: 1 });
  const scaled = (size: number) => Math.round((Math.abs(value) / size) * 10) / 10;
  const index = LEDGER_UNITS.findIndex(({ size }) => Math.abs(value) >= size);
  if (index === -1) return digits.format(value);
  // A figure that rounds up to a thousand of its unit moves up one, so 999,960 reads "1M".
  const unit =
    index > 0 && scaled(LEDGER_UNITS[index].size) >= 1_000
      ? LEDGER_UNITS[index - 1]
      : LEDGER_UNITS[index];
  return `${digits.format(Math.sign(value) * scaled(unit.size))}${unit.suffix}`;
}

/** A cache hit rate (0 to 1) as a whole percent: "77%". */
export function formatCacheHitRate(rate: number): string {
  return formatPct(rate * 100);
}

/**
 * The cost providers reported, or null when none did; a cost under a cent keeps two significant
 * digits instead of rounding to nothing.
 */
export function formatReportedCost(
  row: { reportedCostUsd: number; costReportedRequests: number },
  locale: string,
): string | null {
  if (row.costReportedRequests === 0) return null;
  const value = row.reportedCostUsd;
  return value > 0 && value < 0.01
    ? new Intl.NumberFormat(locale, {
        style: "currency",
        currency: "USD",
        maximumSignificantDigits: 2,
      }).format(value)
    : formatAmount(value, "usd", locale);
}

/** A ledger day ("2026-11-10", UTC) as the app's language writes it: "Nov 10". */
export function formatLedgerDay(day: string, locale: string): string {
  const time = Date.parse(`${day}T00:00:00Z`);
  if (Number.isNaN(time)) return day;
  return new Date(time).toLocaleDateString(locale, {
    month: "short",
    day: "numeric",
    timeZone: "UTC",
  });
}

/** When a host's history starts: "Nov 2, 2026". */
export function formatLedgerDate(time: number, locale: string): string {
  return new Date(time).toLocaleDateString(locale, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

import { formatTokenCount } from "@/components/context-window-meter.utils";
import type { TFunction } from "i18next";
import { i18n } from "@/i18n/i18next";
import {
  formatResetLabel as localizedResetLabel,
  formatRunOutLabel as localizedRunOutLabel,
} from "@/provider-usage/format";
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

export function formatResetLabel(
  iso: string | null | undefined,
  translator: TFunction | boolean = i18n.t.bind(i18n),
  retained = false,
): string | null {
  const t = typeof translator === "function" ? translator : i18n.t.bind(i18n);
  const isRetained = typeof translator === "boolean" ? translator : retained;
  if (isRetained && iso && Date.parse(iso) <= Date.now()) return t("providerUsage.resetTimePassed");
  return localizedResetLabel(iso, t);
}

export function formatRunOutLabel(
  iso: string | null | undefined,
  t: TFunction = i18n.t.bind(i18n),
): string | null {
  return localizedRunOutLabel(iso, t);
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

/** A ledger count in the app's language, compacted past a thousand: "980", "12.3K", "1.2M". */
export function formatLedgerCount(value: number, locale: string): string {
  return new Intl.NumberFormat(locale, {
    notation: value >= 1_000 ? "compact" : "standard",
    maximumFractionDigits: 1,
  }).format(value);
}

/** A ledger token figure, marked "≥" when some calls behind it reported partial usage or none. */
export function formatLedgerTokens(value: number, lowerBound: boolean, locale: string): string {
  const formatted = formatLedgerCount(value, locale);
  return lowerBound ? `≥${formatted}` : formatted;
}

/** A cache hit rate (0 to 1) as a whole percent: "77%". */
export function formatCacheHitRate(rate: number): string {
  return formatPct(rate * 100);
}

/**
 * The cost providers reported, or null when none did. Marked "≥" when only some requests reported
 * one; a cost under a cent keeps two significant digits instead of rounding to nothing.
 */
export function formatReportedCost(
  row: { reportedCostUsd: number; costReportedRequests: number; requests: number },
  locale: string,
): string | null {
  if (row.costReportedRequests === 0) return null;
  const value = row.reportedCostUsd;
  const amount =
    value > 0 && value < 0.01
      ? new Intl.NumberFormat(locale, {
          style: "currency",
          currency: "USD",
          maximumSignificantDigits: 2,
        }).format(value)
      : formatAmount(value, "usd", locale);
  return row.costReportedRequests < row.requests ? `≥${amount}` : amount;
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

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

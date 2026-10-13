import { formatTokenCount } from "@/components/context-window-meter.utils";
import type { TFunction } from "i18next";
import type { ProviderUsageBalanceUnit } from "./types";

export function clampPct(value: number): number {
  return Math.max(0, Math.min(100, value));
}

export function formatPct(value: number): string {
  return `${Math.round(clampPct(value))}%`;
}

function duration(diffMs: number, t: TFunction): string {
  const diffMinutes = Math.floor(diffMs / 60_000);
  const diffHours = Math.floor(diffMinutes / 60);
  const diffDays = Math.floor(diffHours / 24);
  if (diffDays > 0) return t("providerUsage.days", { count: diffDays });
  if (diffHours > 0) return t("providerUsage.hours", { count: diffHours });
  return t("providerUsage.minutes", { count: diffMinutes });
}

function relativeDuration(iso: string, t: TFunction): string | null {
  const diffMs = new Date(iso).getTime() - Date.now();
  if (!Number.isFinite(diffMs)) return null;
  if (diffMs <= 0) return "";
  return duration(diffMs, t);
}

export function formatResetLabel(iso: string | null | undefined, t: TFunction): string | null {
  if (!iso) return null;
  const time = relativeDuration(iso, t);
  if (time === null) return null;
  return time === "" ? t("providerUsage.resettingNow") : t("providerUsage.resets", { time });
}

export function formatRunOutLabel(iso: string | null | undefined, t: TFunction): string | null {
  if (!iso) return null;
  const time = relativeDuration(iso, t);
  if (time === null) return null;
  return time === "" ? t("providerUsage.runsOutNow") : t("providerUsage.runsOut", { time });
}

export function formatAgo(iso: string | null | undefined, t: TFunction): string | null {
  if (!iso) return null;
  const diffMs = Date.now() - new Date(iso).getTime();
  if (!Number.isFinite(diffMs)) return null;
  if (diffMs < 60_000) return t("providerUsage.justNow");
  return t("providerUsage.ago", { time: duration(diffMs, t) });
}

export function formatAmount(value: number, unit: ProviderUsageBalanceUnit): string {
  switch (unit) {
    case "usd":
      return `$${value.toFixed(2)}`;
    case "cny":
      return `CN¥${value.toFixed(2)}`;
    case "tokens":
      return formatTokenCount(value);
    default:
      return value.toLocaleString();
  }
}

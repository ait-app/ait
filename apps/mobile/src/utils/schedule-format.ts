import type { TFunction } from "i18next";
import { i18n } from "@/i18n/i18next";
import type { ScheduleCadence, ScheduleSummary } from "@ait/protocol/schedule/types";
import { validateCronExpression } from "@ait/protocol/schedule/cron-expression";

export type IntervalUnit = "milliseconds" | "seconds" | "minutes" | "hours" | "days";
type CronCadence = Extract<ScheduleCadence, { type: "cron" }>;

const MS_PER_MINUTE = 60_000;
const MS_PER_HOUR = MS_PER_MINUTE * 60;
const MS_PER_DAY = MS_PER_HOUR * 24;

const UNIT_MS: Record<IntervalUnit, number> = {
  milliseconds: 1,
  seconds: 1_000,
  minutes: MS_PER_MINUTE,
  hours: MS_PER_HOUR,
  days: MS_PER_DAY,
};

const DAY_NAMES = [
  "sunday",
  "monday",
  "tuesday",
  "wednesday",
  "thursday",
  "friday",
  "saturday",
] as const;

export function isNewAgentSchedule(schedule: ScheduleSummary): boolean {
  return schedule.target.type === "new-agent";
}

export function scheduleProductName(_schedule: ScheduleSummary, t: TFunction = i18n.t): string {
  return t("schedules.product.schedule");
}

export function resolveScheduleTitle(schedule: ScheduleSummary, t: TFunction = i18n.t): string {
  const name = schedule.name?.trim();
  if (name) {
    return name;
  }
  if (schedule.target.type === "new-agent") {
    const configTitle = schedule.target.config.title?.trim();
    if (configTitle) {
      return configTitle;
    }
  }
  const firstPromptLine = schedule.prompt
    .split("\n")
    .map((line) => line.trim())
    .find((line) => line.length > 0);
  return (
    firstPromptLine ||
    t("schedules.untitled", { product: scheduleProductName(schedule, t).toLowerCase() })
  );
}

export function everyMsToParts(ms: number): { value: number; unit: IntervalUnit } {
  if (!Number.isFinite(ms) || ms <= 0) {
    return { value: 1, unit: "hours" };
  }
  if (ms % MS_PER_DAY === 0) {
    return { value: ms / MS_PER_DAY, unit: "days" };
  }
  if (ms % MS_PER_HOUR === 0) {
    return { value: ms / MS_PER_HOUR, unit: "hours" };
  }
  if (ms % MS_PER_MINUTE === 0) return { value: ms / MS_PER_MINUTE, unit: "minutes" };
  if (ms % 1_000 === 0) return { value: ms / 1_000, unit: "seconds" };
  return { value: ms, unit: "milliseconds" };
}

export function partsToEveryMs(value: number, unit: IntervalUnit): number {
  const normalized = Number.isFinite(value) ? Math.max(1, Math.round(value)) : 1;
  return normalized * UNIT_MS[unit];
}

function formatEvery(everyMs: number, t: TFunction): string {
  const { value, unit } = everyMsToParts(everyMs);
  return t(`schedules.cadence.${unit}`, { count: value });
}

export function formatCadence(cadence: ScheduleCadence, t: TFunction = i18n.t): string {
  if (cadence.type === "once") {
    return t("schedules.cadence.onceAt", {
      time: new Date(cadence.at).toLocaleString(i18n.resolvedLanguage),
    });
  }
  if (cadence.type === "every") {
    return formatEvery(cadence.everyMs, t);
  }
  return describeCron(cadence, t) ?? cadence.expression;
}

/**
 * Humanize a handful of common 5-field cron shapes. Returns null when the
 * expression is valid but not one of the recognized patterns (callers fall
 * back to showing the raw expression).
 */
export function describeCron(cadence: CronCadence, t: TFunction = i18n.t): string | null {
  const trimmed = cadence.expression.trim();
  if (validateCron(trimmed) !== null) {
    return null;
  }

  const [minute, hour, dayOfMonth, month, dayOfWeek] = trimmed.split(/\s+/);

  // Only humanize the simple "fixed time" family: literal minute/hour with the
  // date fields either wildcarded or a recognized day-of-week constraint.
  const minuteNum = Number.parseInt(minute, 10);
  const isLiteralMinute = /^\d+$/.test(minute);
  const isWildcardMonth = month === "*";
  const isWildcardDom = dayOfMonth === "*";

  if (minute === "*" && hour === "*" && isWildcardMonth && isWildcardDom && dayOfWeek === "*") {
    return t("schedules.cadence.presets.every-minute");
  }

  if (!isLiteralMinute || !isWildcardMonth || !isWildcardDom) {
    return null;
  }

  // "Every hour" / "Every hour at :MM"
  if (hour === "*") {
    if (dayOfWeek !== "*") {
      return null;
    }
    return minuteNum === 0
      ? t("schedules.cadence.presets.every-hour")
      : t("schedules.cadence.hourAt", { minute: pad2(minuteNum) });
  }

  if (!/^\d+$/.test(hour)) {
    return null;
  }
  const time = `${pad2(Number.parseInt(hour, 10))}:${pad2(minuteNum)}`;
  const timezone = cadence.timezone ?? "UTC";
  const dayLabel = describeCronDay(dayOfWeek, t);
  return dayLabel ? t("schedules.cadence.at", { day: dayLabel, time, timezone }) : null;
}

function describeCronDay(dayOfWeek: string, t: TFunction): string | null {
  if (dayOfWeek === "*") {
    return t("schedules.cadence.daily");
  }
  if (dayOfWeek === "1-5") {
    return t("schedules.cadence.weekdays");
  }
  if (dayOfWeek === "0,6" || dayOfWeek === "6,0") {
    return t("schedules.cadence.weekends");
  }
  if (/^\d$/.test(dayOfWeek)) {
    const day = DAY_NAMES[Number.parseInt(dayOfWeek, 10)];
    return day ? t(`schedules.cadence.${day}`) : null;
  }
  return null;
}

export function validateCron(expr: string, t: TFunction = i18n.t): string | null {
  const trimmed = expr.trim();
  if (!trimmed) {
    return t("schedules.cronErrors.required");
  }

  const error = validateCronExpression(trimmed);
  if (!error) {
    return null;
  }
  if (error === "Cron expressions must have 5 fields") {
    return t("schedules.cronErrors.fields");
  }
  const match =
    /^Invalid cron (minute|hour|day-of-month|month|day-of-week) (step|range|value|field)$/.exec(
      error,
    );
  if (match) {
    const field = match[1] as "minute" | "hour" | "day-of-month" | "month" | "day-of-week";
    const kind = match[2] as "step" | "range" | "value" | "field";
    return t(`schedules.cronErrors.${field}.${kind}`);
  }
  return t("schedules.cronErrors.invalid");
}

function pad2(value: number): string {
  return value < 10 ? `0${value}` : String(value);
}

/**
 * Forward-relative description of the next run, e.g. "in 3h", "in 2d", "soon".
 * Returns "" when there is no scheduled next run.
 */
export function formatNextRun(iso: string | null, t: TFunction = i18n.t): string {
  if (!iso) {
    return "";
  }
  const target = new Date(iso).getTime();
  if (Number.isNaN(target)) {
    return "";
  }

  const diffMs = target - Date.now();
  if (diffMs <= 0) {
    return t("schedules.time.soon");
  }
  if (diffMs < MS_PER_MINUTE) {
    return t("schedules.time.soon");
  }
  if (diffMs < MS_PER_HOUR) {
    return t("schedules.time.nextMinutes", { count: Math.round(diffMs / MS_PER_MINUTE) });
  }
  if (diffMs < MS_PER_DAY) {
    return t("schedules.time.nextHours", { count: Math.round(diffMs / MS_PER_HOUR) });
  }
  return t("schedules.time.nextDays", { count: Math.round(diffMs / MS_PER_DAY) });
}

/** Localized timestamps for schedule creation and run history. */
export function formatScheduleTimeAgo(
  date: Date,
  t: TFunction = i18n.t,
  locale = i18n.resolvedLanguage,
): string {
  const diffMs = Date.now() - date.getTime();
  if (diffMs < 10_000) return t("schedules.time.now");
  if (diffMs < MS_PER_MINUTE)
    return t("schedules.time.secondsAgo", { count: Math.floor(diffMs / 1000) });
  if (diffMs < MS_PER_HOUR)
    return t("schedules.time.minutesAgo", { count: Math.floor(diffMs / MS_PER_MINUTE) });
  if (diffMs < MS_PER_DAY)
    return t("schedules.time.hoursAgo", { count: Math.floor(diffMs / MS_PER_HOUR) });
  if (diffMs < 7 * MS_PER_DAY)
    return t("schedules.time.daysAgo", { count: Math.floor(diffMs / MS_PER_DAY) });
  return date.toLocaleDateString(locale, { month: "short", day: "numeric" });
}

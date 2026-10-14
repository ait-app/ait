import type { TFunction } from "i18next";
import { i18n } from "@/i18n/i18next";
import type { ScheduleCadence } from "@ait/protocol/schedule/types";

export type ScheduleFormCadence = Exclude<ScheduleCadence, { type: "once" }>;

type CronCadence = Extract<ScheduleCadence, { type: "cron" }>;

export interface CadencePresetOption {
  id: "every-minute" | "every-hour" | "daily-9" | "weekdays-9" | "mondays-9";
  label: string;
  expression: string;
}

export const CUSTOM_CRON_PRESET_ID = "custom";

export const CADENCE_PRESET_OPTIONS: CadencePresetOption[] = [
  { id: "every-minute", label: "Every minute", expression: "* * * * *" },
  { id: "every-hour", label: "Every hour", expression: "0 * * * *" },
  { id: "daily-9", label: "Daily 9:00", expression: "0 9 * * *" },
  { id: "weekdays-9", label: "Weekdays 9:00", expression: "0 9 * * 1-5" },
  { id: "mondays-9", label: "Mondays 9:00", expression: "0 9 * * 1" },
];

export function resolveCronPresetId(cadence: CronCadence): string {
  const expression = cadence.expression.trim();
  return (
    CADENCE_PRESET_OPTIONS.find((option) => option.expression === expression)?.id ??
    CUSTOM_CRON_PRESET_ID
  );
}

export function getCadencePresetOptions(t: TFunction = i18n.t): CadencePresetOption[] {
  return CADENCE_PRESET_OPTIONS.map((option) => ({
    ...option,
    label: t(`schedules.cadence.presets.${option.id}`),
  }));
}

export function resolveCronPresetDisplay(
  cadence: CronCadence,
  t: TFunction = i18n.t,
): { label: string } {
  return {
    label:
      getCadencePresetOptions(t).find((option) => option.id === resolveCronPresetId(cadence))
        ?.label ?? t("schedules.cadence.custom"),
  };
}

export function normalizeScheduleFormCadence(
  cadence: ScheduleCadence,
  timezone: string,
): ScheduleFormCadence {
  if (cadence.type === "once") return { type: "every", everyMs: 3_600_000 };
  return cadence.type === "cron" ? { ...cadence, timezone: cadence.timezone ?? timezone } : cadence;
}

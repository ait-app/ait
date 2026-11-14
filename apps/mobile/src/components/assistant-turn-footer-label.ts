import type { TFunction } from "i18next";
import { formatDuration } from "@/utils/time";

export function getTurnDurationLabel(durationMs: number, t: TFunction): string {
  return t("message.turnFooter.workedFor", { duration: formatDuration(durationMs) });
}

/** What a screen reader announces for a footer that can show both the duration and the end time. */
export function getTurnFooterAccessibilityLabel(
  durationLabel: string,
  timestampLabel: string,
  t: TFunction,
): string {
  return t("message.turnFooter.endedAt", { duration: durationLabel, timestamp: timestampLabel });
}

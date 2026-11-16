import type { TFunction } from "i18next";
import {
  COUNTED_CHECK_PRESENTATIONS,
  type CheckPresentationCounts,
  type CountedCheckPresentation,
} from "./check-presentation";

const CHECK_PRESENTATION_COUNT_KEYS = {
  success: "sidebar.workspace.checks.passed",
  failure: "sidebar.workspace.checks.failed",
  warning: "sidebar.workspace.checks.warning",
  actionRequired: "sidebar.workspace.checks.actionRequired",
  manual: "sidebar.workspace.checks.manual",
  pending: "sidebar.workspace.checks.pending",
} as const satisfies Record<CountedCheckPresentation, string>;

function getCheckPresentationCountLabel(
  presentation: CountedCheckPresentation,
  count: number,
  t: TFunction,
): string {
  return t(CHECK_PRESENTATION_COUNT_KEYS[presentation], { count });
}

export function formatCheckPresentationCountsLabel(
  counts: CheckPresentationCounts,
  heading: string,
  t: TFunction,
): string {
  const labels = COUNTED_CHECK_PRESENTATIONS.flatMap((presentation) => {
    const count = counts[presentation];
    return count > 0 ? [getCheckPresentationCountLabel(presentation, count, t)] : [];
  });
  return [heading, ...labels].join(", ");
}

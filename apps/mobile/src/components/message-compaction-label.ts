import { i18n } from "@/i18n/i18next";
import type { CompactionTimelineItem } from "@ait/protocol/agent-types";

export type CompactionMarkerLabelInput = Pick<
  CompactionTimelineItem,
  "status" | "trigger" | "preTokens"
>;

export function getCompactionMarkerLabel({
  status,
  trigger,
  preTokens,
}: CompactionMarkerLabelInput): string {
  if (status === "loading") return i18n.t("message.compaction.loading");
  if (status === "failed") return i18n.t("message.compaction.failed");
  if (trigger === "auto") return i18n.t("message.compaction.auto");
  if (trigger === "manual") return i18n.t("message.compaction.manual");
  if (preTokens) {
    return i18n.t("message.compaction.withTokens", {
      tokens: Math.round(preTokens / 1000),
    });
  }
  return i18n.t("message.compaction.completed");
}

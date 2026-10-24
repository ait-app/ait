import type { TFunction } from "i18next";
import { formatThinkingOptionLabel } from "@/agent-controls/labels";

interface ControlOption {
  id: string;
  label?: string | null;
}

const MODE_LABEL_KEYS = {
  "Plan Mode": "planMode",
  "Always Ask": "alwaysAsk",
  "Accept File Edits": "acceptEdits",
  "Auto mode": "auto",
  Bypass: "bypass",
  "Default Permissions": "defaultPermissions",
  "Auto-review": "autoReview",
  "Full Access": "fullAccess",
  Plan: "plan",
  "Allow All": "allowAll",
  Build: "build",
  "Write Approval": "writeApproval",
  Default: "default",
} as const;

export function scheduleModeLabel(option: ControlOption, t: TFunction): string {
  const label = option.label ?? option.id;
  const key = MODE_LABEL_KEYS[label as keyof typeof MODE_LABEL_KEYS];
  return key ? t(`schedules.modeLabels.${key}`) : label;
}

export function scheduleThinkingLabel(option: ControlOption, t: TFunction): string {
  const label = formatThinkingOptionLabel(option, t);
  switch (label.toLowerCase()) {
    case "none":
      return t("schedules.thinking.none");
    case "minimal":
      return t("schedules.thinking.minimal");
    case "low":
      return t("schedules.thinking.low");
    case "medium":
      return t("schedules.thinking.medium");
    case "high":
      return t("schedules.thinking.high");
    case "extra high":
      return t("schedules.thinking.xhigh");
    case "max":
      return t("schedules.thinking.max");
    case "off":
      return t("schedules.thinking.off");
    case "on":
      return t("schedules.thinking.on");
    default:
      return label;
  }
}

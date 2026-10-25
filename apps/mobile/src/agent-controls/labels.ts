import type { TFunction } from "i18next";
import { i18n } from "@/i18n/i18next";

interface ControlLabelInput {
  id: string;
  label?: string | null;
}

const MODE_LABEL_KEYS = new Map([
  ["plan mode", "schedules.modeLabels.planMode"],
  ["always ask", "schedules.modeLabels.alwaysAsk"],
  ["accept file edits", "schedules.modeLabels.acceptEdits"],
  ["auto mode", "schedules.modeLabels.auto"],
  ["bypass", "schedules.modeLabels.bypass"],
  ["default permissions", "schedules.modeLabels.defaultPermissions"],
  ["auto-review", "schedules.modeLabels.autoReview"],
  ["full access", "schedules.modeLabels.fullAccess"],
  ["plan", "schedules.modeLabels.plan"],
  ["allow all", "schedules.modeLabels.allowAll"],
  ["build", "schedules.modeLabels.build"],
  ["write approval", "schedules.modeLabels.writeApproval"],
  ["default", "schedules.modeLabels.default"],
  ["allow file writes", "agentControls.mode.allowFileWrites"],
  ["ask and remember", "agentControls.mode.askAndRemember"],
]);

const THINKING_LABEL_KEYS = new Map([
  ["none", "schedules.thinking.none"],
  ["off", "schedules.thinking.off"],
  ["on", "schedules.thinking.on"],
  ["minimal", "schedules.thinking.minimal"],
  ["low", "schedules.thinking.low"],
  ["medium", "schedules.thinking.medium"],
  ["high", "schedules.thinking.high"],
  ["xhigh", "schedules.thinking.xhigh"],
  ["extrahigh", "schedules.thinking.xhigh"],
  ["max", "schedules.thinking.max"],
]);

const FEATURE_LABEL_KEYS = new Map([
  ["autoCompaction", "agentControls.features.autoCompaction"],
  ["autoRetry", "agentControls.features.autoRetry"],
  ["fastMode", "agentControls.features.fastMode"],
  ["slowMode", "agentControls.features.slowMode"],
  ["service_tier", "agentControls.speed.title"],
]);

function sentenceCase(value: string): string {
  if (!value) {
    return value;
  }
  return value.charAt(0).toUpperCase() + value.slice(1).toLowerCase();
}

function splitCompactLabel(value: string, splitHyphen: boolean): string {
  const separatorPattern = splitHyphen ? /[_-]+/g : /_+/g;

  return value
    .replace(separatorPattern, " ")
    .replace(/([a-z])([A-Z])/g, "$1 $2")
    .replace(/([A-Z]+)([A-Z][a-z])/g, "$1 $2")
    .replace(/\s+/g, " ")
    .trim();
}

function formatControlLabel(option: ControlLabelInput, splitHyphen: boolean): string {
  const rawLabel = (option.label ?? option.id).trim();
  return sentenceCase(splitCompactLabel(rawLabel, splitHyphen));
}

export function formatAgentModeLabel(mode: ControlLabelInput, t: TFunction = i18n.t): string {
  const label = formatControlLabel(mode, mode.label == null);
  const key = MODE_LABEL_KEYS.get(label.toLowerCase());
  return key ? sentenceCase(t(key)) : label;
}

export function formatThinkingOptionLabel(
  option: ControlLabelInput,
  t: TFunction = i18n.t,
): string {
  const rawLabel = (option.label ?? option.id).trim();
  const compactLabel = rawLabel.replace(/[\s_-]+/g, "").toLowerCase();
  const key = THINKING_LABEL_KEYS.get(compactLabel);
  return key ? t(key) : formatControlLabel(option, true);
}

export function formatAgentFeatureLabel(feature: ControlLabelInput, t: TFunction = i18n.t): string {
  const key = FEATURE_LABEL_KEYS.get(feature.id);
  return key ? t(key) : (feature.label ?? feature.id);
}

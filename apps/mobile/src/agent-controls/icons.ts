import type { ComponentType } from "react";
import type { AgentFeature } from "@ait/protocol/agent-types";
import {
  Bot,
  Brain,
  Hammer,
  ListTodo,
  Minimize2,
  RotateCcw,
  Settings2,
  Shield,
  ShieldAlert,
  ShieldCheck,
  ShieldEllipsis,
  ShieldOff,
  ShieldPlus,
  ShieldQuestionMark,
  Turtle,
  Zap,
} from "lucide-react-native";
import { getModeVisuals, type AgentProviderDefinition } from "@ait/protocol/provider-manifest";

export interface AgentControlIconProps {
  size: number;
  color: string;
}

export type AgentControlIcon = ComponentType<AgentControlIconProps>;

export const ThinkingIcon = Brain;
export const PlanModeIcon = ListTodo;

const MODE_ICONS: Record<string, AgentControlIcon> = {
  Bot,
  Hammer,
  Shield,
  ShieldAlert,
  ShieldCheck,
  ShieldEllipsis,
  ShieldOff,
  ShieldPlus,
  ShieldQuestionMark,
};

const FEATURE_ICONS: Record<string, AgentControlIcon> = {
  "list-todo": ListTodo,
  "shield-check": ShieldCheck,
  zap: Zap,
  "minimize-2": Minimize2,
  "rotate-ccw": RotateCcw,
  turtle: Turtle,
};

const FEATURE_ID_ICONS: Record<string, AgentControlIcon> = {
  autoCompaction: Minimize2,
  autoRetry: RotateCcw,
  fastMode: Zap,
  slowMode: Turtle,
};

const PERMISSION_ICONS: Record<string, AgentControlIcon> = {
  allow: ShieldCheck,
  ask: ShieldQuestionMark,
  deny: ShieldOff,
};

export function getAgentModeIcon(
  provider: string,
  modeId: string,
  providerDefinitions: AgentProviderDefinition[],
): AgentControlIcon {
  const icon = getModeVisuals(provider, modeId, providerDefinitions)?.icon;
  return (icon ? MODE_ICONS[icon] : undefined) ?? Bot;
}

export function getAgentFeatureIcon(icon?: string, featureId?: string): AgentControlIcon {
  return (
    (icon ? FEATURE_ICONS[icon] : undefined) ??
    (featureId ? FEATURE_ID_ICONS[featureId] : undefined) ??
    Settings2
  );
}

export function getAgentFeatureValueIcon(
  feature: Pick<AgentFeature, "id" | "icon">,
  value: unknown,
): AgentControlIcon {
  if (feature.id === "permission") {
    return (typeof value === "string" ? PERMISSION_ICONS[value] : undefined) ?? Shield;
  }
  return getAgentFeatureIcon(feature.icon, feature.id);
}

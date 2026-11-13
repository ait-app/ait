import type { AgentFeature, AgentModelDefinition } from "@ait/protocol/agent-types";
import type { AgentConfigApply, ProviderPreset } from "@ait/protocol/messages";
import { findModelByReference } from "@/provider-selection/model-catalog";

const ROLE_PREFIX = "modelRole.";
const INTERNAL_PRESET_FIELDS = new Set(["modelPreset", "modelPresetThinking", "modelPresetRoles"]);

/** A preset replaces the model, thinking and complete role snapshot in one operation. */
export interface ModelPresetConfiguration extends AgentConfigApply {
  modelId: string;
  thinkingOptionId: string | null;
  featureValues: Record<string, unknown>;
}

/** The browser receives an application target, never an AgentProfile disguised as a preset. */
export interface ModelPresetControls {
  cwd?: string | null;
  apply: (provider: string, configuration: ModelPresetConfiguration) => void;
}

/** Whether the provider's discovered controls support model-role assignments. */
export function supportsModelRoles(features: readonly AgentFeature[] | undefined): boolean {
  return Boolean(
    features?.some((feature) => feature.type === "select" && feature.id.startsWith(ROLE_PREFIX)),
  );
}

/** Unknown stored selections remain editable, but cannot promise an applicable configuration. */
export function isModelPresetAvailable(
  preset: ProviderPreset,
  models: readonly AgentModelDefinition[],
): boolean {
  const model = findModelByReference(models, preset.model);
  if (!model || model.isSelectable === false) return false;
  return (
    !preset.thinkingOptionId ||
    preset.thinkingOptionId === model.defaultThinkingOptionId ||
    Boolean(model.thinkingOptions?.some((option) => option.id === preset.thinkingOptionId))
  );
}

/**
 * Expand the same saved snapshot the settings page edits. Roles omitted from a snapshot return
 * to provider inheritance; an explicit `unset` remains explicit. Permissions and tools never
 * become part of this patch, even if an older preset happened to store those fields.
 */
export function modelPresetConfiguration(
  preset: ProviderPreset,
  features: readonly AgentFeature[],
): ModelPresetConfiguration {
  const featureValues: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(preset.featureValues ?? {})) {
    if (key.startsWith(ROLE_PREFIX)) featureValues[key] = value;
  }
  return withModelPresetRoleDefaults(
    {
      modelId: preset.model,
      thinkingOptionId: preset.thinkingOptionId ?? null,
      featureValues,
    },
    features,
  );
}

/** Clear omitted roles from both the live catalog and the agent's older stored role set. */
export function withModelPresetRoleDefaults(
  configuration: ModelPresetConfiguration,
  features: readonly AgentFeature[],
): ModelPresetConfiguration {
  const defaults: Record<string, unknown> = {};
  for (const feature of features) {
    if (feature.type !== "select" || !feature.id.startsWith(ROLE_PREFIX)) continue;
    defaults[feature.id] = feature.options.find((option) => option.isDefault)?.id ?? "native";
  }
  return { ...configuration, featureValues: { ...defaults, ...configuration.featureValues } };
}

/** Preserve unrelated current settings while replacing the complete model-role snapshot. */
export function mergeModelPresetFeatureValues(
  current: Record<string, unknown> | undefined,
  configuration: ModelPresetConfiguration,
): Record<string, unknown> {
  const retained = Object.fromEntries(
    Object.entries(current ?? {}).filter(
      ([key]) => !key.startsWith(ROLE_PREFIX) && !INTERNAL_PRESET_FIELDS.has(key),
    ),
  );
  return { ...retained, ...configuration.featureValues };
}

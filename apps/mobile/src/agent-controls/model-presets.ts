import type { AgentFeature, AgentModelDefinition } from "@ait/protocol/agent-types";
import type { AgentConfigApply, ProviderPreset } from "@ait/protocol/messages";
import { findModelByReference } from "@/provider-selection/model-catalog";

const ROLE_PREFIX = "modelRole.";
const FALLBACK_PREFIX = "modelFallback.";
const INTERNAL_PRESET_FIELDS = new Set([
  "modelPreset",
  "modelPresetThinking",
  "modelPresetRoles",
  "modelPresetFallbacks",
]);

/** A preset replaces the model, thinking and complete role/fallback snapshot in one operation. */
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

/** Whether the provider's discovered role controls advertise ordered fallback chains. */
export function supportsModelFallbacks(features: readonly AgentFeature[] | undefined): boolean {
  return Boolean(
    features?.some(
      (feature) =>
        feature.type === "select" &&
        feature.id.startsWith(ROLE_PREFIX) &&
        feature.options.find((option) => option.id === "native")?.metadata?.supportsFallbacks ===
          true,
    ),
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
 * Expand the same saved snapshot the settings page edits. Roles and fallbacks omitted from a
 * snapshot return to provider inheritance; an explicit `unset` remains explicit. Permissions and
 * tools never become part of this patch, even if an older preset happened to store those fields.
 */
export function modelPresetConfiguration(
  preset: ProviderPreset,
  features: readonly AgentFeature[],
  models: readonly AgentModelDefinition[] = [],
): ModelPresetConfiguration {
  const resolvedModel = models.length > 0 ? findModelByReference(models, preset.model) : null;
  const featureValues: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(preset.featureValues ?? {})) {
    if (key === "modelRole.default") continue;
    if (key.startsWith(ROLE_PREFIX) || key.startsWith(FALLBACK_PREFIX)) {
      featureValues[key] = value;
    }
  }
  return withModelPresetRoleDefaults(
    {
      modelId: resolvedModel?.id ?? preset.model,
      thinkingOptionId: preset.thinkingOptionId ?? null,
      featureValues,
    },
    features,
  );
}

/** Clear omitted roles and fallbacks from both the live catalog and older stored role sets. */
export function withModelPresetRoleDefaults(
  configuration: ModelPresetConfiguration,
  features: readonly AgentFeature[],
): ModelPresetConfiguration {
  const defaults: Record<string, unknown> = {};
  const hasFallbacks = supportsModelFallbacks(features);
  for (const feature of features) {
    if (feature.type !== "select" || !feature.id.startsWith(ROLE_PREFIX)) continue;
    defaults[feature.id] = feature.options.find((option) => option.isDefault)?.id ?? "native";
    if (hasFallbacks) {
      defaults[`${FALLBACK_PREFIX}${feature.id.slice(ROLE_PREFIX.length)}`] = "native";
    }
  }
  if (hasFallbacks) {
    defaults[`${FALLBACK_PREFIX}default`] = "native";
  }
  const featureValues = { ...defaults, ...configuration.featureValues };
  if ("modelRole.default" in featureValues) {
    featureValues["modelRole.default"] = defaults["modelRole.default"] ?? "native";
  }
  return { ...configuration, featureValues };
}

/** Preserve unrelated current settings while replacing the complete role and fallback snapshot. */
export function mergeModelPresetFeatureValues(
  current: Record<string, unknown> | undefined,
  configuration: ModelPresetConfiguration,
): Record<string, unknown> {
  const retained = Object.fromEntries(
    Object.entries(current ?? {}).filter(
      ([key]) =>
        !key.startsWith(ROLE_PREFIX) &&
        !key.startsWith(FALLBACK_PREFIX) &&
        !INTERNAL_PRESET_FIELDS.has(key),
    ),
  );
  return { ...retained, ...configuration.featureValues };
}

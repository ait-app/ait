import type { AgentFeature } from "@ait/protocol/agent-types";
import { AgentConfigApplySchema, type AgentConfigApply } from "@ait/protocol/messages";
import { i18n } from "@/i18n/i18next";

export const MODEL_PRESET_FIELDS = ["modelPresetThinking", "modelPresetRoles"] as const;

/** Prefix that marks a native role selector, matching the adapter's own `modelRole.` keys. */
const ROLE_PREFIX = "modelRole.";

/** The selector that clears a role, so a preset's absent roles cannot leak the previous value. */
const ROLE_CLEARED = "unset";

/** Model configuration belongs to the existing model picker, not the general feature tray. */
export function isModelConfigurationFeature(feature: AgentFeature): boolean {
  return (
    feature.type === "select" &&
    (feature.id === "modelPreset" || feature.id.startsWith("modelRole."))
  );
}

/**
 * Expand one preset into the role configuration it actually applies.
 *
 * A preset only stores the roles it names. Applying it still clears every other role, so a preset
 * written without an advisor must disable a leftover advisor rather than leave it in place. Both
 * applying and matching go through this function: matching the narrow stored map instead would
 * keep a preset selected after the user manually enabled a role it never named.
 */
function impliedFeatureValues(
  features: readonly AgentFeature[] | undefined,
  named: Record<string, unknown> | undefined,
): Record<string, unknown> {
  const cleared: Record<string, unknown> = {};
  for (const feature of features ?? []) {
    if (feature.id.startsWith(ROLE_PREFIX) && !(named && feature.id in named)) {
      cleared[feature.id] = ROLE_CLEARED;
    }
  }
  return { ...cleared, ...named };
}

/** A native preset is one configuration change; it is never a sequence of role setters. */
export function modelPresetConfiguration(
  features: readonly AgentFeature[] | undefined,
  featureId: string,
  value: unknown,
): AgentConfigApply | null {
  if (featureId !== "modelPreset" || value === "__custom__") return null;
  const feature = features?.find((item) => item.id === featureId);
  const option =
    feature?.type === "select" ? feature.options.find((item) => item.id === value) : null;
  if (option?.metadata?.error) throw new Error(String(option.metadata.error));
  const parsed = AgentConfigApplySchema.safeParse(option?.metadata?.configuration);
  if (!parsed.success || !parsed.data.modelId) {
    throw new Error(i18n.t("providerControl.presetUnavailable"));
  }
  // The applied patch states every role, including the ones the preset does not name: an omitted
  // field would keep the session's previous assignment instead of clearing it.
  parsed.data.featureValues = impliedFeatureValues(features, parsed.data.featureValues);
  return parsed.data;
}

/** Recompute the preset label from the actual model, effort and role values after manual edits. */
export function withModelPresetSelection(
  features: AgentFeature[],
  current: {
    modelId?: string | null;
    thinkingOptionId?: string | null;
    featureValues?: Record<string, unknown>;
  },
): AgentFeature[] {
  const values: Record<string, unknown> = Object.fromEntries(
    features.map((feature) => [feature.id, feature.value]),
  );
  Object.assign(values, current.featureValues);
  return features.map((feature) => {
    if (feature.id !== "modelPreset" || feature.type !== "select") return feature;
    const selected = feature.options.find((option) => {
      const parsed = AgentConfigApplySchema.safeParse(option.metadata?.configuration);
      if (!parsed.success) return false;
      const config = parsed.data;
      if (
        config.modelId !== current.modelId ||
        (config.thinkingOptionId ?? null) !== (current.thinkingOptionId || null)
      ) {
        return false;
      }
      // Compare what applying this preset would set, not the narrower stored map: a role the
      // preset never named is cleared on apply, so it must also read as different from any
      // value the user has since chosen by hand.
      return Object.entries(impliedFeatureValues(features, config.featureValues)).every(
        ([key, value]) =>
          key === "modelPreset" ||
          MODEL_PRESET_FIELDS.some((field) => field === key) ||
          Object.is(values[key], value),
      );
    });
    return { ...feature, value: selected?.id ?? "__custom__" };
  });
}

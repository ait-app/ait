import type { AgentFeature } from "@ait/protocol/agent-types";
import { AgentConfigApplySchema, type AgentConfigApply } from "@ait/protocol/messages";
import { i18n } from "@/i18n/i18next";

export const MODEL_PRESET_FIELDS = ["modelPresetThinking", "modelPresetRoles"] as const;

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
  if (!parsed.success || !parsed.data.featureValues || !parsed.data.modelId) {
    throw new Error(i18n.t("providerControl.presetUnavailable"));
  }
  for (const field of features ?? []) {
    if (field.id.startsWith("modelRole.") && !(field.id in parsed.data.featureValues))
      parsed.data.featureValues[field.id] = "unset";
  }
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
      return (
        config.modelId === current.modelId &&
        (config.thinkingOptionId ?? null) === (current.thinkingOptionId || null) &&
        Object.entries(config.featureValues ?? {}).every(
          ([key, value]) =>
            key === "modelPreset" ||
            MODEL_PRESET_FIELDS.some((field) => field === key) ||
            Object.is(values[key], value),
        )
      );
    });
    return { ...feature, value: selected?.id ?? "__custom__" };
  });
}

import type { AgentFeature } from "@ait/protocol/agent-types";
import {
  isModelConfigurationFeature,
  withModelPresetSelection,
} from "@/agent-controls/model-presets";

/**
 * Refresh native role choices while preserving the saved next-turn selections.
 *
 * `saved` is what the session persisted; `discovered` is the provider's current catalog. Stored
 * features keep their order and values, and anything the catalog knows that the session does not
 * is appended. Without a stored list the catalog is the whole answer, so a session that saved no
 * controls still shows the ones the provider discovers now.
 */
export function mergeRoleChoices(
  saved: AgentFeature[] | undefined,
  discovered: AgentFeature[] | undefined,
): AgentFeature[] | undefined {
  if (!saved) return discovered;
  if (!discovered) return saved;
  const roles = new Map(
    discovered
      .filter((feature) => feature.id.startsWith("modelRole.") || feature.id === "modelPreset")
      .map((feature) => [feature.id, feature]),
  );
  const merged = saved.map((feature) => {
    const current = roles.get(feature.id);
    roles.delete(feature.id);
    if (current?.type !== "select" || feature.type !== "select") return feature;
    const selected = feature.options.find((option) => option.id === feature.value);
    const options =
      selected && !current.options.some((option) => option.id === selected.id)
        ? [...current.options, selected]
        : current.options;
    return { ...current, value: feature.value, options };
  });
  return [...merged, ...roles.values()];
}

/**
 * The features a live session must show: stored order and values, refreshed choices, and a preset
 * label recomputed from the effective selection.
 *
 * Discovery is unconditional. A session saved before a preset existed has no `modelPreset` feature
 * to merge into, so gating on the stored list would hide the preset the user just created until
 * the session was restarted; the catalog entry is appended instead.
 */
export function liveAgentFeatures(
  saved: AgentFeature[] | undefined,
  discovered: AgentFeature[] | undefined,
  current: { modelId?: string | null; thinkingOptionId?: string | null },
): AgentFeature[] | undefined {
  const merged = mergeRoleChoices(saved, discovered);
  if (!merged?.some(isModelConfigurationFeature)) return merged;
  return withModelPresetSelection(merged, current);
}

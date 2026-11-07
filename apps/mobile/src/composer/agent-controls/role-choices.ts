import type { AgentFeature } from "@ait/protocol/agent-types";

/** Refresh native role choices while preserving the saved next-turn selections. */
export function mergeRoleChoices(
  saved: AgentFeature[] | undefined,
  discovered: AgentFeature[] | undefined,
): AgentFeature[] | undefined {
  if (!saved || !discovered) return saved;
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

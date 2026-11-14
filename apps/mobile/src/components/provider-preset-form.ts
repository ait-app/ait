import type {
  AgentFeature,
  AgentModelDefinition,
  AgentSelectOption,
} from "@ait/protocol/agent-types";
import type { ProviderPreset } from "@ait/protocol/messages";

export type RoleFeature = Extract<AgentFeature, { type: "select" }>;
export interface PresetRoleRow {
  id: string;
  role: string;
  model: string;
  thinking: string;
}
interface RoleModelChoice {
  id: string;
  label: string;
  thinkingOptions: AgentSelectOption[];
}

const ROLE_PREFIX = "modelRole.";
const THINKING_SUFFIX = /^(.*):(off|minimal|low|medium|high|xhigh|max)$/;

/** Decode only native thinking suffixes; model names containing colons remain intact. */
export function splitRoleSelector(value: string): { model: string; thinking: string } {
  const match = THINKING_SUFFIX.exec(value);
  return match ? { model: match[1]!, thinking: match[2]! } : { model: value, thinking: "" };
}

/** A serialized OMP `[provider, model]` route, or null for ordinary aliases and qualified names. */
function parseModelRoute(model: string): [string, string] | null {
  try {
    const route: unknown = JSON.parse(model);
    if (
      Array.isArray(route) &&
      route.length === 2 &&
      route.every((part) => typeof part === "string")
    ) {
      return [route[0], route[1]];
    }
  } catch {
    // Providers also use ordinary aliases and qualified model names.
  }
  return null;
}

/** A route's readable fallback; serialized OMP arrays never appear in the preset list. */
export function presetModelLabel(model: string, models: readonly AgentModelDefinition[]): string {
  const known = models.find((entry) => entry.id === model);
  if (known) return known.label;
  return parseModelRoute(model)?.[1] ?? model;
}

/** Ait-managed providers keep their own catalog identifiers. */
function nativeModelSelector(model: string): string {
  return parseModelRoute(model)?.join("/") ?? model;
}

export function presetRoleFeatures(features: readonly AgentFeature[]): RoleFeature[] {
  return features.filter(
    (feature): feature is RoleFeature =>
      feature.type === "select" &&
      feature.id.startsWith(ROLE_PREFIX) &&
      feature.id !== "modelRole.default",
  );
}

/** Native options own role compatibility; the chat model catalog supplies readable labels. */
export function roleModelChoices(
  feature: RoleFeature | undefined,
  models: readonly AgentModelDefinition[],
): RoleModelChoice[] {
  const choices = new Map<string, RoleModelChoice>();
  for (const option of feature?.options ?? []) {
    if (option.id === "native" || option.id === "unset") continue;
    const { model, thinking } = splitRoleSelector(option.id);
    const known = models.find((entry) => nativeModelSelector(entry.id) === model);
    const choice = choices.get(model) ?? {
      id: model,
      label:
        known?.label ??
        option.label
          .split(" · ")[0]!
          .replace(/ \((?:off|minimal|low|medium|high|xhigh|max)\)$/, ""),
      thinkingOptions: [],
    };
    if (thinking && !choice.thinkingOptions.some((entry) => entry.id === thinking)) {
      choice.thinkingOptions.push({ id: thinking, label: thinking });
    }
    choices.set(model, choice);
  }
  return [...choices.values()];
}

/** Missing rows inherit on apply; native/unset are explicit, separate choices. */
export function decodePresetRoles(preset: ProviderPreset): PresetRoleRow[] {
  return Object.entries(preset.featureValues ?? {})
    .filter(
      ([key, value]) =>
        key.startsWith(ROLE_PREFIX) && key !== "modelRole.default" && typeof value === "string",
    )
    .map(([role, value], index) => ({
      id: `stored-${index}`,
      role,
      ...splitRoleSelector(value as string),
    }));
}

/** Preserve unrelated and opaque values while rebuilding editable role records. */
export function encodePresetRoles(
  original: ProviderPreset,
  rows: readonly PresetRoleRow[],
): Record<string, unknown> {
  const editableRoles = new Set(decodePresetRoles(original).map((row) => row.role));
  const values = Object.fromEntries(
    Object.entries(original.featureValues ?? {}).filter(([key]) => !editableRoles.has(key)),
  );
  const seen = new Set<string>();
  for (const row of rows) {
    if (!row.role.startsWith(ROLE_PREFIX) || !row.model || seen.has(row.role)) {
      throw new Error("Invalid or duplicate model role");
    }
    seen.add(row.role);
    values[row.role] =
      row.thinking && !["native", "unset"].includes(row.model)
        ? `${row.model}:${row.thinking}`
        : row.model;
  }
  return values;
}

/** Native routing facts come from metadata, never from parsing a provider's display sentence. */
export function nativeRolePreview(
  feature: RoleFeature | undefined,
  models: readonly AgentModelDefinition[],
) {
  const metadata = feature?.options.find((option) => option.id === "native")?.metadata;
  const selector = metadata?.resolvedSelector;
  if (typeof selector !== "string" || selector === "auto") return null;
  const split = splitRoleSelector(selector);
  const known = models.find((model) => nativeModelSelector(model.id) === split.model);
  return { ...split, label: known?.label ?? split.model };
}

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

export interface PresetFallbackItem {
  id: string;
  model: string;
  thinking: string;
}

export interface RoleModelChoice {
  id: string;
  label: string;
  description?: string;
  vendor: string | null;
  thinkingOptions: AgentSelectOption[];
}

export interface IndexedPresetModel {
  id: string;
  selector: string;
  vendor: string | null;
  label: string;
  qualifiedLabel: string;
  description?: string;
  entry: AgentModelDefinition;
}

export interface ModelCatalogIndex {
  byId: Map<string, IndexedPresetModel>;
  bySelector: Map<string, IndexedPresetModel>;
  providers: string[];
  modelOptions: { id: string; value: string; label: string; description?: string }[];
}

const ROLE_PREFIX = "modelRole.";
const FALLBACK_PREFIX = "modelFallback.";
const THINKING_SUFFIX = /^(.*):(auto|off|minimal|low|medium|high|xhigh|max)$/;
const LABEL_EFFORT_SUFFIX = / \((?:auto|off|minimal|low|medium|high|xhigh|max)\)$/;

function isModelCatalogIndex(
  input: readonly AgentModelDefinition[] | ModelCatalogIndex,
): input is ModelCatalogIndex {
  return !Array.isArray(input) && "byId" in input && "bySelector" in input;
}

function ensureCatalogIndex(
  input: readonly AgentModelDefinition[] | ModelCatalogIndex,
): ModelCatalogIndex {
  return isModelCatalogIndex(input) ? input : buildModelCatalogIndex(input);
}

/** Decode only native thinking suffixes; model names containing colons remain intact. */
export function splitRoleSelector(value: string): { model: string; thinking: string } {
  const match = THINKING_SUFFIX.exec(value);
  return match ? { model: match[1]!, thinking: match[2]! } : { model: value, thinking: "" };
}

/** A serialized OMP `[provider, model]` route, or null for ordinary aliases and qualified names. */
export function parseModelRoute(model: string): [string, string] | null {
  if (!model.startsWith("[")) return null;
  try {
    const route: unknown = JSON.parse(model);
    if (
      Array.isArray(route) &&
      route.length === 2 &&
      route.every((part) => typeof part === "string" && part.length > 0)
    ) {
      return [route[0]!, route[1]!];
    }
  } catch {
    // Providers also use ordinary aliases and qualified model names.
  }
  return null;
}

/** Prefix `label` with upstream `vendor` unless empty or already prefixed. */
export function qualifyModelLabel(vendor: string | null | undefined, label: string): string {
  const normalizedVendor = vendor?.trim() ?? "";
  const normalizedLabel = label.trim();
  if (!normalizedVendor) return normalizedLabel;
  if (!normalizedLabel) return normalizedVendor;
  const lowerLabel = normalizedLabel.toLowerCase();
  const lowerVendor = normalizedVendor.toLowerCase();
  if (
    lowerLabel.startsWith(`${lowerVendor} / `) ||
    lowerLabel.startsWith(`${lowerVendor}/`) ||
    lowerLabel === lowerVendor
  ) {
    return normalizedLabel;
  }
  return `${normalizedVendor} / ${normalizedLabel}`;
}

function extractVendorFromSelector(selector: string): { vendor: string | null; rest: string } {
  if (selector.startsWith("@")) return { vendor: null, rest: selector };
  const slash = selector.indexOf("/");
  if (slash <= 0 || slash >= selector.length - 1) {
    return { vendor: null, rest: selector };
  }
  const vendor = selector.slice(0, slash).trim();
  const rest = selector.slice(slash + 1).trim();
  if (!vendor || !rest || /\s/.test(vendor)) {
    return { vendor: null, rest: selector };
  }
  return { vendor, rest };
}

function extractModelVendor(entry: AgentModelDefinition): {
  vendor: string | null;
  selector: string;
  routeDescription?: string;
} {
  const metadataProvider =
    typeof entry.metadata?.provider === "string" ? entry.metadata.provider.trim() : "";
  const metadataModelId =
    typeof entry.metadata?.modelId === "string" ? entry.metadata.modelId.trim() : "";
  const route = parseModelRoute(entry.id);
  if (route) {
    const vendor = metadataProvider || route[0];
    const modelId = metadataModelId || route[1];
    const selector = `${route[0]}/${route[1]}`;
    return {
      vendor,
      selector,
      routeDescription: modelId !== entry.label ? `${vendor}/${modelId}` : undefined,
    };
  }
  if (metadataProvider) {
    const modelId = metadataModelId || entry.id;
    return {
      vendor: metadataProvider,
      selector: entry.id.includes("/") ? entry.id : `${metadataProvider}/${modelId}`,
      routeDescription: modelId !== entry.label ? `${metadataProvider}/${modelId}` : undefined,
    };
  }
  const split = extractVendorFromSelector(entry.id);
  if (split.vendor) {
    return {
      vendor: split.vendor,
      selector: entry.id,
      routeDescription: split.rest !== entry.label ? entry.id : undefined,
    };
  }
  return { vendor: null, selector: entry.id };
}

/** Build an O(M) lookup index once per model catalog snapshot. */
export function buildModelCatalogIndex(models: readonly AgentModelDefinition[]): ModelCatalogIndex {
  const byId = new Map<string, IndexedPresetModel>();
  const bySelector = new Map<string, IndexedPresetModel>();
  const providerSet = new Set<string>();
  const modelOptions: ModelCatalogIndex["modelOptions"] = [];

  for (const entry of models) {
    const { vendor, selector, routeDescription } = extractModelVendor(entry);
    if (vendor) providerSet.add(vendor);
    const qualifiedLabel = qualifyModelLabel(vendor, entry.label);
    const rawDescription = entry.description?.trim() || routeDescription;
    const compactEquivalent = vendor ? `${vendor}/${entry.label}` : entry.label;
    const description =
      rawDescription &&
      rawDescription !== qualifiedLabel &&
      rawDescription.toLowerCase() !== compactEquivalent.toLowerCase()
        ? rawDescription
        : undefined;
    const indexed: IndexedPresetModel = {
      id: entry.id,
      selector,
      vendor,
      label: entry.label,
      qualifiedLabel,
      ...(description ? { description } : {}),
      entry,
    };
    byId.set(entry.id, indexed);
    bySelector.set(selector, indexed);
    if (Array.isArray(entry.aliases)) {
      for (const alias of entry.aliases) {
        if (alias && !byId.has(alias)) byId.set(alias, indexed);
      }
    }
    modelOptions.push({
      id: entry.id,
      value: entry.id,
      label: qualifiedLabel,
      ...(description ? { description } : {}),
    });
  }

  return {
    byId,
    bySelector,
    providers: [...providerSet].sort((left, right) => left.localeCompare(right)),
    modelOptions,
  };
}

/** Resolve human-readable label (with upstream supplier when known) and optional route hint. */
export function presetModelDisplay(
  model: string,
  modelsOrIndex: readonly AgentModelDefinition[] | ModelCatalogIndex,
): { label: string; description?: string; vendor: string | null } {
  const index = ensureCatalogIndex(modelsOrIndex);
  const known = index.byId.get(model) ?? index.bySelector.get(model);
  if (known) {
    return {
      label: known.qualifiedLabel,
      ...(known.description ? { description: known.description } : {}),
      vendor: known.vendor,
    };
  }
  const route = parseModelRoute(model);
  if (route) {
    const [vendor, modelId] = route;
    const leaf = modelId.split("/").filter(Boolean).pop() ?? modelId;
    const label = qualifyModelLabel(vendor, leaf);
    const full = `${vendor}/${modelId}`;
    return {
      label,
      ...(modelId.includes("/") ? { description: full } : {}),
      vendor,
    };
  }
  const split = extractVendorFromSelector(model);
  if (split.vendor) {
    if (split.rest === "*") {
      return { label: `${split.vendor} / *`, vendor: split.vendor };
    }
    const leaf = split.rest.split("/").filter(Boolean).pop() ?? split.rest;
    return {
      label: qualifyModelLabel(split.vendor, leaf),
      ...(split.rest.includes("/") ? { description: model } : {}),
      vendor: split.vendor,
    };
  }
  return { label: model, vendor: null };
}

/** A route's readable label including upstream vendor; serialized OMP arrays never leak raw JSON. */
export function presetModelLabel(
  model: string,
  modelsOrIndex: readonly AgentModelDefinition[] | ModelCatalogIndex,
): string {
  return presetModelDisplay(model, modelsOrIndex).label;
}

/** Format a selector that may carry `:effort` into a readable vendor-qualified summary. */
export function formatSelectorSummary(
  selector: string,
  modelsOrIndex: readonly AgentModelDefinition[] | ModelCatalogIndex,
): string {
  const { model, thinking } = splitRoleSelector(selector);
  const label = presetModelLabel(model, modelsOrIndex);
  return thinking ? `${label} (${thinking})` : label;
}

export function defaultRoleFeature(features: readonly AgentFeature[]): RoleFeature | undefined {
  return features.find(
    (feature): feature is RoleFeature =>
      feature.type === "select" && feature.id === "modelRole.default",
  );
}

export function presetRoleFeatures(features: readonly AgentFeature[]): RoleFeature[] {
  return features.filter(
    (feature): feature is RoleFeature =>
      feature.type === "select" &&
      feature.id.startsWith(ROLE_PREFIX) &&
      feature.id !== "modelRole.default",
  );
}

export function presetSupportsFallbacks(
  features: readonly AgentFeature[],
  preset?: ProviderPreset,
): boolean {
  if (
    features.some(
      (feature) =>
        feature.type === "select" &&
        feature.id.startsWith(ROLE_PREFIX) &&
        feature.options.find((option) => option.id === "native")?.metadata?.supportsFallbacks ===
          true,
    )
  ) {
    return true;
  }
  return Object.keys(preset?.featureValues ?? {}).some((key) => key.startsWith(FALLBACK_PREFIX));
}

/** Native options own role compatibility; the indexed catalog supplies vendor-qualified labels. */
export function roleModelChoices(
  feature: RoleFeature | undefined,
  modelsOrIndex: readonly AgentModelDefinition[] | ModelCatalogIndex,
): RoleModelChoice[] {
  const index = ensureCatalogIndex(modelsOrIndex);
  const choices = new Map<string, RoleModelChoice>();

  for (const option of feature?.options ?? []) {
    if (option.id === "native" || option.id === "unset") continue;
    const { model, thinking } = splitRoleSelector(option.id);
    let choice = choices.get(model);
    if (!choice) {
      const known = index.bySelector.get(model) ?? index.byId.get(model);
      const optionVendor =
        typeof option.metadata?.provider === "string"
          ? option.metadata.provider.trim()
          : extractVendorFromSelector(model).vendor;
      const rawLabel = option.label
        .split(" · ")[0]!
        .replace(/ \(images\)$/, "")
        .replace(LABEL_EFFORT_SUFFIX, "");
      const vendor = known?.vendor ?? (optionVendor || null);
      const label = known ? known.qualifiedLabel : qualifyModelLabel(vendor, rawLabel);
      const description =
        known?.description ??
        (vendor && model !== `${vendor}/${rawLabel}` && model.includes("/") ? model : undefined);
      choice = {
        id: model,
        label,
        ...(description ? { description } : {}),
        vendor,
        thinkingOptions: [],
      };
      const metadataEfforts = option.metadata?.efforts;
      if (Array.isArray(metadataEfforts)) {
        for (const effort of metadataEfforts) {
          if (typeof effort === "string" && effort) {
            choice.thinkingOptions.push({ id: effort, label: effort });
          }
        }
      }
      choices.set(model, choice);
    }
    if (thinking && !choice.thinkingOptions.some((entry) => entry.id === thinking)) {
      choice.thinkingOptions.push({ id: thinking, label: thinking });
    }
  }

  return [...choices.values()];
}

/** Candidate options for fallback selectors: role-compatible models plus provider wildcards. */
export function roleFallbackChoices(
  Choices: readonly RoleModelChoice[],
  modelsIndex: ModelCatalogIndex,
): RoleModelChoice[] {
  if (Choices.length > 0) {
    const vendors = new Set<string>();
    for (const item of Choices) {
      if (item.vendor) vendors.add(item.vendor);
    }
    const wildcards: RoleModelChoice[] = [...vendors]
      .sort((a, b) => a.localeCompare(b))
      .map((vendor) => ({
        id: `${vendor}/*`,
        label: `${vendor} / *`,
        vendor,
        thinkingOptions: [],
      }));
    return [...Choices, ...wildcards];
  }
  const fallbackFromCatalog: RoleModelChoice[] = modelsIndex.modelOptions.map((item) => {
    const indexed = modelsIndex.byId.get(item.id);
    return {
      id: indexed?.selector ?? item.id,
      label: item.label,
      ...(item.description ? { description: item.description } : {}),
      vendor: indexed?.vendor ?? null,
      thinkingOptions: (indexed?.entry.thinkingOptions ?? []).map((opt) => ({
        id: opt.id,
        label: opt.label,
      })),
    };
  });
  const wildcards: RoleModelChoice[] = modelsIndex.providers.map((vendor) => ({
    id: `${vendor}/*`,
    label: `${vendor} / *`,
    vendor,
    thinkingOptions: [],
  }));
  return [...fallbackFromCatalog, ...wildcards];
}

/** Inherited OMP fallback chains exposed on the role's `native` option metadata. */
export function inheritedRoleFallbacks(
  feature: RoleFeature | undefined,
  modelsOrIndex: readonly AgentModelDefinition[] | ModelCatalogIndex,
): string[] {
  const raw = feature?.options.find((option) => option.id === "native")?.metadata
    ?.inheritedFallbacks;
  if (!Array.isArray(raw)) return [];
  const index = ensureCatalogIndex(modelsOrIndex);
  return raw
    .filter((entry): entry is string => typeof entry === "string" && entry.trim().length > 0)
    .map((selector) => formatSelectorSummary(selector, index));
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

/** Decode ordered per-role fallback chains (`modelFallback.<roleId>`). */
export function decodePresetFallbacks(
  preset: ProviderPreset,
): Record<string, PresetFallbackItem[]> {
  const result: Record<string, PresetFallbackItem[]> = {};
  let sequence = 0;
  for (const [key, value] of Object.entries(preset.featureValues ?? {})) {
    if (!key.startsWith(FALLBACK_PREFIX) || !Array.isArray(value)) continue;
    const roleId = key.slice(FALLBACK_PREFIX.length);
    if (!roleId || !value.every((entry) => typeof entry === "string")) continue;
    result[roleId] = (value as string[]).map((selector) => ({
      id: `fallback-${sequence++}`,
      ...splitRoleSelector(selector),
    }));
  }
  return result;
}

/** Preserve unrelated and opaque values while rebuilding editable role and fallback records. */
export function encodePresetRoles(
  original: ProviderPreset,
  rows: readonly PresetRoleRow[],
  fallbacks?: Record<string, readonly PresetFallbackItem[]>,
): Record<string, unknown> {
  const editableRoles = new Set(decodePresetRoles(original).map((row) => row.role));
  const editableFallbacks = fallbacks
    ? new Set(
        Object.keys(decodePresetFallbacks(original)).map((roleId) => `${FALLBACK_PREFIX}${roleId}`),
      )
    : new Set<string>();
  const values = Object.fromEntries(
    Object.entries(original.featureValues ?? {}).filter(
      ([key]) => !editableRoles.has(key) && !editableFallbacks.has(key),
    ),
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
  if (fallbacks) {
    for (const [roleId, items] of Object.entries(fallbacks)) {
      if (!roleId || items.length === 0) continue;
      values[`${FALLBACK_PREFIX}${roleId}`] = items.map((item) => {
        if (!item.model) {
          throw new Error("Invalid fallback model selector");
        }
        const canHaveThinking =
          Boolean(item.thinking) &&
          !item.model.startsWith("@") &&
          !item.model.endsWith("/*") &&
          !["native", "unset"].includes(item.model);
        return canHaveThinking ? `${item.model}:${item.thinking}` : item.model;
      });
    }
  }
  return values;
}

/** Native routing facts come from metadata, never from parsing a provider's display sentence. */
export function nativeRolePreview(
  feature: RoleFeature | undefined,
  modelsOrIndex: readonly AgentModelDefinition[] | ModelCatalogIndex,
) {
  const metadata = feature?.options.find((option) => option.id === "native")?.metadata;
  const selector = metadata?.resolvedSelector;
  if (typeof selector !== "string" || selector === "auto") return null;
  const split = splitRoleSelector(selector);
  const label = presetModelLabel(split.model, modelsOrIndex);
  return { ...split, label };
}

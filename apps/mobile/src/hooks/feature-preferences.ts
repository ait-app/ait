import type { AgentFeature } from "@ait/protocol/agent-types";

const ROLE_PREFIX = "modelRole.";
const FALLBACK_PREFIX = "modelFallback.";

function allowedFallbackFeatureIds(features: readonly AgentFeature[]): Set<string> {
  const allowed = new Set<string>();
  let supportsFallbacks = false;
  for (const feature of features) {
    if (feature.type !== "select" || !feature.id.startsWith(ROLE_PREFIX)) continue;
    if (
      feature.options.find((option) => option.id === "native")?.metadata?.supportsFallbacks === true
    ) {
      supportsFallbacks = true;
      allowed.add(`${FALLBACK_PREFIX}${feature.id.slice(ROLE_PREFIX.length)}`);
    }
  }
  if (supportsFallbacks) {
    allowed.add(`${FALLBACK_PREFIX}default`);
  }
  return allowed;
}

export function pruneFeatureValues(
  featureValues: Record<string, unknown>,
  features: AgentFeature[],
): Record<string, unknown> {
  const allowedFeatureIds = new Set(features.map((feature) => feature.id));
  const fallbackFeatureIds = allowedFallbackFeatureIds(features);
  let changed = false;
  const next: Record<string, unknown> = {};

  for (const [featureId, value] of Object.entries(featureValues)) {
    if (!allowedFeatureIds.has(featureId) && !fallbackFeatureIds.has(featureId)) {
      changed = true;
      continue;
    }
    next[featureId] = value;
  }

  return changed ? next : featureValues;
}

export function applyFeatureValues(
  features: AgentFeature[],
  featureValues: Record<string, unknown>,
): AgentFeature[] {
  if (Object.keys(featureValues).length === 0) {
    return features;
  }

  return features.map((feature) => {
    if (!Object.prototype.hasOwnProperty.call(featureValues, feature.id)) {
      return feature;
    }

    return {
      ...feature,
      value: featureValues[feature.id],
    } as AgentFeature;
  });
}

export function resolveFeatureValues(args: {
  features: AgentFeature[];
  persistedFeatureValues: Record<string, unknown>;
  localFeatureValues: Record<string, unknown>;
}): Record<string, unknown> {
  const next: Record<string, unknown> = {};

  for (const feature of args.features) {
    if (Object.prototype.hasOwnProperty.call(args.localFeatureValues, feature.id)) {
      next[feature.id] = args.localFeatureValues[feature.id];
      continue;
    }
    if (Object.prototype.hasOwnProperty.call(args.persistedFeatureValues, feature.id)) {
      next[feature.id] = args.persistedFeatureValues[feature.id];
    }
  }

  for (const fallbackId of allowedFallbackFeatureIds(args.features)) {
    if (Object.prototype.hasOwnProperty.call(args.localFeatureValues, fallbackId)) {
      next[fallbackId] = args.localFeatureValues[fallbackId];
      continue;
    }
    if (Object.prototype.hasOwnProperty.call(args.persistedFeatureValues, fallbackId)) {
      next[fallbackId] = args.persistedFeatureValues[fallbackId];
    }
  }

  return next;
}

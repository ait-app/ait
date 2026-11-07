import type { AgentFeature } from "@ait/protocol/agent-types";
import { MODEL_PRESET_FIELDS } from "@/agent-controls/model-presets";

function configurableIds(features: AgentFeature[]): Set<string> {
  const ids = new Set(features.map((feature) => feature.id));
  if (ids.has("modelPreset")) for (const key of MODEL_PRESET_FIELDS) ids.add(key);
  return ids;
}

export function pruneFeatureValues(
  featureValues: Record<string, unknown>,
  features: AgentFeature[],
): Record<string, unknown> {
  const allowedFeatureIds = configurableIds(features);
  let changed = false;
  const next: Record<string, unknown> = {};

  for (const [featureId, value] of Object.entries(featureValues)) {
    if (!allowedFeatureIds.has(featureId)) {
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
    if (feature.id === "modelPreset" && featureValues[feature.id] == null) return feature;
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

  for (const id of configurableIds(args.features)) {
    if (Object.prototype.hasOwnProperty.call(args.localFeatureValues, id)) {
      next[id] = args.localFeatureValues[id];
      continue;
    }
    if (Object.prototype.hasOwnProperty.call(args.persistedFeatureValues, id)) {
      next[id] = args.persistedFeatureValues[id];
    }
  }

  return next;
}

import { describe, expect, it } from "vitest";
import type { AgentFeature } from "@ait/protocol/agent-types";
import { modelPresetConfiguration, withModelPresetSelection } from "./model-presets";
import {
  applyFeatureValues,
  pruneFeatureValues,
  resolveFeatureValues,
} from "@/hooks/feature-preferences";

const configuration = {
  modelId: '["test","model"]',
  thinkingOptionId: "high",
  featureValues: {
    modelPreset: null,
    "modelRole.default": "test/model",
    "modelRole.advisor": "unset",
    modelPresetThinking: "auto",
    modelPresetRoles: JSON.stringify({ default: "test/model" }),
  },
};
const features: AgentFeature[] = [
  {
    type: "select",
    id: "modelPreset",
    label: "Preset",
    value: "__custom__",
    options: [
      { id: "__custom__", label: "Custom" },
      { id: "work", label: "Work", metadata: { configuration } },
      { id: "broken", label: "Broken", metadata: { error: "Unavailable model" } },
    ],
  },
  { type: "select", id: "modelRole.default", label: "Default", value: "test/model", options: [] },
  { type: "select", id: "modelRole.advisor", label: "Advisor", value: "unset", options: [] },
];

describe("OMP model presets", () => {
  it("uses one complete configuration and preserves native ownership snapshots", () => {
    expect(modelPresetConfiguration(features, "modelPreset", "work")).toEqual(configuration);
    const values = pruneFeatureValues(
      { ...configuration.featureValues, oldFeature: true },
      features,
    );
    expect(values).toEqual(configuration.featureValues);
    expect(
      resolveFeatureValues({ features, localFeatureValues: values, persistedFeatureValues: {} }),
    ).toEqual(values);
    expect(applyFeatureValues(features, values)[0].value).toBe("__custom__");
  });
  it("recomputes the selected preset after individual model, effort or role changes", () => {
    expect(withModelPresetSelection(features, configuration)[0].value).toBe("work");
    for (const changed of [
      { modelId: "another" },
      { thinkingOptionId: "low" },
      { featureValues: { "modelRole.advisor": "test/model" } },
    ]) {
      expect(withModelPresetSelection(features, { ...configuration, ...changed })[0].value).toBe(
        "__custom__",
      );
    }
  });
  it("keeps native discovery failures visible without producing partial changes", () => {
    expect(() => modelPresetConfiguration(features, "modelPreset", "broken")).toThrow(
      "Unavailable model",
    );
    expect(() => modelPresetConfiguration(features, "modelPreset", "gone")).toThrow("unavailable");
    expect(modelPresetConfiguration(features, "autoRetry", true)).toBeNull();
    expect(modelPresetConfiguration(features, "modelPreset", "__custom__")).toBeNull();
  });
});

describe("modelPresetConfiguration with a model-only preset", () => {
  // A preset created from the settings editor may name only a model and an optional thinking
  // level; the synthesized configuration then carries no feature map at all.
  const modelOnly: AgentFeature = {
    type: "select",
    id: "modelPreset",
    label: "Preset",
    value: "__custom__",
    options: [
      { id: "__custom__", label: "Custom" },
      {
        id: "plain",
        label: "Plain",
        metadata: { configuration: { modelId: "claude-opus-5", thinkingOptionId: "high" } },
      },
    ],
  };
  const role: AgentFeature = {
    type: "select",
    id: "modelRole.advisor",
    label: "Advisor",
    value: "native",
    options: [{ id: "native", label: "Use Claude setting" }],
  };

  it("applies instead of reporting the preset as unavailable", () => {
    const applied = modelPresetConfiguration([modelOnly, role], "modelPreset", "plain");
    expect(applied?.modelId).toBe("claude-opus-5");
    expect(applied?.thinkingOptionId).toBe("high");
  });

  it("still blocks roles the preset does not name, so an old override cannot leak", () => {
    const applied = modelPresetConfiguration([modelOnly, role], "modelPreset", "plain");
    expect(applied?.featureValues).toEqual({ "modelRole.advisor": "unset" });
  });
});

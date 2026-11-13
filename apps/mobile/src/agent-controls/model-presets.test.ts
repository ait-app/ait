import { describe, expect, it } from "vitest";
import type { AgentFeature } from "@ait/protocol/agent-types";
import {
  mergeModelPresetFeatureValues,
  modelPresetConfiguration,
  supportsModelRoles,
  withModelPresetRoleDefaults,
} from "./model-presets";

const features: AgentFeature[] = [
  {
    type: "select",
    id: "modelRole.advisor",
    label: "Advisor",
    value: "old-model",
    options: [{ id: "native", label: "Inherit", isDefault: true }],
  },
  {
    type: "select",
    id: "modelRole.worker",
    label: "Worker",
    value: "old-model",
    options: [{ id: "native", label: "Inherit", isDefault: true }],
  },
  { type: "toggle", id: "autoRetry", label: "Retry", value: true },
];

describe("provider model preset snapshots", () => {
  it("clears an older agent's unresolved roles before the daemon merges the new snapshot", () => {
    const snapshot = modelPresetConfiguration(
      { name: "Quick", model: "fast", featureValues: { "modelRole.worker": "unset" } },
      features,
    );
    const complete = withModelPresetRoleDefaults(snapshot, [
      ...features,
      { type: "select", id: "modelRole.obsolete", label: "Obsolete", value: "old", options: [] },
    ]);
    expect(complete.featureValues).toEqual({
      "modelRole.worker": "unset",
      "modelRole.advisor": "native",
      "modelRole.obsolete": "native",
    });
    expect(complete).not.toHaveProperty("modeId");
  });
  it("replaces named roles, restores omitted roles to inheritance and keeps explicit unassigned", () => {
    expect(
      modelPresetConfiguration(
        {
          name: "Quick",
          model: "fast",
          featureValues: { "modelRole.worker": "unset", autoRetry: false },
        },
        features,
      ),
    ).toEqual({
      modelId: "fast",
      thinkingOptionId: null,
      featureValues: { "modelRole.advisor": "native", "modelRole.worker": "unset" },
    });
  });

  it("preserves unrelated current settings and removes stale roles and private preset markers", () => {
    const configuration = modelPresetConfiguration(
      {
        name: "Work",
        model: "large",
        thinkingOptionId: "high",
        featureValues: { "modelRole.advisor": "large" },
      },
      features,
    );
    expect(
      mergeModelPresetFeatureValues(
        {
          autoRetry: true,
          tools: "restricted",
          "modelRole.obsolete": "old",
          modelPreset: "previous",
          modelPresetRoles: "private",
        },
        configuration,
      ),
    ).toEqual({
      autoRetry: true,
      tools: "restricted",
      "modelRole.advisor": "large",
      "modelRole.worker": "native",
    });
    expect(configuration.thinkingOptionId).toBe("high");
  });

  it("uses the discovered inheritance default instead of guessing a provider's role choices", () => {
    expect(
      modelPresetConfiguration({ name: "Work", model: "large" }, [
        {
          type: "select",
          id: "modelRole.worker",
          label: "Worker",
          value: "old",
          options: [{ id: "inherit", label: "Inherit", isDefault: true }],
        },
      ]).featureValues,
    ).toEqual({ "modelRole.worker": "inherit" });
    expect(supportsModelRoles(features)).toBe(true);
    expect(supportsModelRoles([features[2]])).toBe(false);
    expect(supportsModelRoles(undefined)).toBe(false);
  });
});

import { describe, expect, it } from "vitest";
import type { AgentFeature, AgentModelDefinition } from "@ait/protocol/agent-types";
import {
  mergeModelPresetFeatureValues,
  modelPresetConfiguration,
  supportsModelFallbacks,
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

const fallbackFeatures: AgentFeature[] = [
  {
    type: "select",
    id: "modelRole.default",
    label: "Default",
    value: "old-default",
    options: [
      { id: "native", label: "Inherit", isDefault: true, metadata: { supportsFallbacks: true } },
    ],
  },
  {
    type: "select",
    id: "modelRole.worker",
    label: "Worker",
    value: "old-model",
    options: [
      { id: "native", label: "Inherit", isDefault: true, metadata: { supportsFallbacks: true } },
    ],
  },
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

  it("canonicalizes model aliases, resets modelRole.default to native and carries fallback chains", () => {
    const models: AgentModelDefinition[] = [
      { provider: "omp", id: '["openai-codex","gpt-6-astra"]', aliases: ["astra"], label: "Astra" },
    ];
    expect(supportsModelFallbacks(fallbackFeatures)).toBe(true);
    expect(supportsModelFallbacks(features)).toBe(false);

    expect(
      modelPresetConfiguration(
        {
          name: "GPT",
          model: "astra",
          thinkingOptionId: "auto",
          featureValues: {
            "modelRole.default": "stale/default",
            "modelRole.worker": "unset",
            "modelFallback.default": ["tokencake-2/deepseek-v4:auto"],
            autoRetry: false,
          },
        },
        fallbackFeatures,
        models,
      ),
    ).toEqual({
      modelId: '["openai-codex","gpt-6-astra"]',
      thinkingOptionId: "auto",
      featureValues: {
        "modelRole.default": "native",
        "modelRole.worker": "unset",
        "modelFallback.default": ["tokencake-2/deepseek-v4:auto"],
        "modelFallback.worker": "native",
      },
    });
  });

  it("preserves unrelated current settings and removes stale roles, fallbacks and private markers", () => {
    const configuration = modelPresetConfiguration(
      {
        name: "Work",
        model: "large",
        thinkingOptionId: "high",
        featureValues: {
          "modelRole.advisor": "large",
          "modelFallback.default": ["backup/model"],
        },
      },
      features,
    );
    expect(
      mergeModelPresetFeatureValues(
        {
          autoRetry: true,
          tools: "restricted",
          "modelRole.obsolete": "old",
          "modelFallback.obsolete": ["stale/fallback"],
          modelPreset: "previous",
          modelPresetRoles: "private",
          modelPresetFallbacks: "private",
        },
        configuration,
      ),
    ).toEqual({
      autoRetry: true,
      tools: "restricted",
      "modelRole.advisor": "large",
      "modelRole.worker": "native",
      "modelFallback.default": ["backup/model"],
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

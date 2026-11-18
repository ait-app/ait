import { describe, expect, it } from "vitest";

import { pruneFeatureValues, resolveFeatureValues } from "./feature-preferences";

describe("feature-preferences", () => {
  const features = [
    {
      type: "toggle" as const,
      id: "fast_mode",
      label: "Fast",
      value: false,
    },
    {
      type: "toggle" as const,
      id: "plan_mode",
      label: "Plan",
      value: false,
    },
  ];

  it("restores persisted values for available features", () => {
    expect(
      resolveFeatureValues({
        features,
        persistedFeatureValues: {
          fast_mode: true,
          unknown_feature: true,
        },
        localFeatureValues: {},
      }),
    ).toEqual({
      fast_mode: true,
    });
  });

  it("prefers local values over persisted values", () => {
    expect(
      resolveFeatureValues({
        features,
        persistedFeatureValues: {
          fast_mode: true,
          plan_mode: false,
        },
        localFeatureValues: {
          fast_mode: false,
        },
      }),
    ).toEqual({
      fast_mode: false,
      plan_mode: false,
    });
  });

  it("retains and resolves modelFallback chains when role controls advertise fallback support", () => {
    const roleFeatures = [
      {
        type: "select" as const,
        id: "modelRole.task",
        label: "Task",
        value: "native",
        options: [
          {
            id: "native",
            label: "Inherit",
            isDefault: true,
            metadata: { supportsFallbacks: true },
          },
        ],
      },
    ];
    expect(
      pruneFeatureValues(
        {
          "modelRole.task": "openai-codex/gpt-6-astra:auto",
          "modelFallback.default": ["tokencake-2/*"],
          "modelFallback.task": ["openai-codex/gpt-6-astra"],
          "modelFallback.ghost": ["invalid/role"],
        },
        roleFeatures,
      ),
    ).toEqual({
      "modelRole.task": "openai-codex/gpt-6-astra:auto",
      "modelFallback.default": ["tokencake-2/*"],
      "modelFallback.task": ["openai-codex/gpt-6-astra"],
    });
    expect(
      resolveFeatureValues({
        features: roleFeatures,
        persistedFeatureValues: { "modelFallback.default": ["old/fallback"] },
        localFeatureValues: { "modelFallback.default": ["new/fallback"] },
      }),
    ).toEqual({
      "modelFallback.default": ["new/fallback"],
    });
  });
});

import { expect, it } from "vitest";
import {
  buildModelCatalogIndex,
  decodePresetFallbacks,
  decodePresetRoles,
  encodePresetRoles,
  inheritedRoleFallbacks,
  nativeRolePreview,
  presetModelLabel,
  roleFallbackChoices,
  roleModelChoices,
  splitRoleSelector,
} from "./provider-preset-form";

it("separates native effort suffixes including auto and off without splitting model colons", () => {
  expect(splitRoleSelector("gateway:region/model:high")).toEqual({
    model: "gateway:region/model",
    thinking: "high",
  });
  expect(splitRoleSelector("openai-codex/gpt-6-astra:auto")).toEqual({
    model: "openai-codex/gpt-6-astra",
    thinking: "auto",
  });
  expect(splitRoleSelector("gateway:model-v2")).toEqual({
    model: "gateway:model-v2",
    thinking: "",
  });
});

it("round trips explicit models, role aliases, fallbacks, defaults and unrelated features", () => {
  const preset = {
    name: "work",
    model: "sonnet",
    featureValues: {
      "modelRole.advisor": "provider/model:high",
      "modelRole.task": "@smol",
      "modelRole.memory": "native",
      "modelFallback.default": ["openai-codex/gpt-6-astra:auto", "tokencake-2/*"],
      "modelFallback.task": ["@advisor"],
      fastMode: true,
    },
  };
  expect(
    encodePresetRoles(preset, decodePresetRoles(preset), decodePresetFallbacks(preset)),
  ).toEqual(preset.featureValues);
});

it("preserves explicit unassigned roles and omits removed roles and fallbacks for inheritance", () => {
  const original = {
    name: "work",
    model: "sonnet",
    featureValues: {
      "modelRole.advisor": "opus",
      "modelRole.task": "unset",
      "modelFallback.default": ["provider/backup:low"],
    },
  };
  expect(decodePresetRoles(original)).toHaveLength(2);
  expect(decodePresetFallbacks(original).default).toEqual([
    { id: "fallback-0", model: "provider/backup", thinking: "low" },
  ]);
  expect(
    encodePresetRoles(original, decodePresetRoles(original), decodePresetFallbacks(original)),
  ).toEqual(original.featureValues);
  expect(encodePresetRoles(original, [], {})).toEqual({});
});

it("retains unresolved selectors, opaque roles and default-role data while editing other fields", () => {
  const original = {
    name: "work",
    model: "unknown-model",
    featureValues: {
      "modelRole.future": "unknown:model:turbo",
      "modelRole.structured": { model: "future-model" },
      "modelRole.default": "old-default",
      fastMode: true,
    },
  };
  expect(encodePresetRoles(original, decodePresetRoles(original))).toEqual(original.featureValues);
  expect(encodePresetRoles(original, [])).toEqual({
    "modelRole.structured": { model: "future-model" },
    "modelRole.default": "old-default",
    fastMode: true,
  });
});

it("does not attach thinking to default, unassigned, alias or wildcard sentinel values", () => {
  expect(
    encodePresetRoles(
      { name: "p", model: "m" },
      [{ id: "a", role: "modelRole.advisor", model: "native", thinking: "high" }],
      {
        default: [
          { id: "f1", model: "vendor/*", thinking: "high" },
          { id: "f2", model: "@advisor", thinking: "low" },
        ],
      },
    ),
  ).toEqual({
    "modelRole.advisor": "native",
    "modelFallback.default": ["vendor/*", "@advisor"],
  });
});

it("qualifies multi-vendor models with upstream supplier while keeping single-vendor models clean", () => {
  const index = buildModelCatalogIndex([
    {
      provider: "omp",
      id: '["tokencake-2","deepseek/deepseek-v4.1-flash-1017"]',
      label: "deepseek-v4.1-flash-1017",
      description: "tokencake-2/deepseek/deepseek-v4.1-flash-1017",
      metadata: { provider: "tokencake-2", modelId: "deepseek/deepseek-v4.1-flash-1017" },
    },
    { provider: "claude", id: "claude-opus-5", label: "Opus" },
  ]);
  expect(presetModelLabel('["tokencake-2","deepseek/deepseek-v4.1-flash-1017"]', index)).toBe(
    "tokencake-2 / deepseek-v4.1-flash-1017",
  );
  expect(presetModelLabel("claude-opus-5", index)).toBe("Opus");
  expect(presetModelLabel('["native","other"]', index)).toBe("native / other");

  const feature = {
    type: "select" as const,
    id: "modelRole.advisor",
    value: "native",
    label: "Advisor",
    options: [
      {
        id: "native",
        label: "Provider default",
        metadata: { inheritedFallbacks: ["tokencake-2/deepseek/deepseek-v4.1-flash-1017:auto"] },
      },
      { id: "unset", label: "Unassigned" },
      {
        id: "native/model",
        label: "Raw · native/model",
        metadata: { provider: "native", modelId: "model", efforts: ["auto", "high"] },
      },
    ],
  };
  const choices = roleModelChoices(feature, [
    { provider: "omp", id: '["native","model"]', label: "Human model" },
  ]);
  expect(choices).toEqual([
    {
      id: "native/model",
      label: "native / Human model",
      description: "native/model",
      vendor: "native",
      thinkingOptions: [
        { id: "auto", label: "auto" },
        { id: "high", label: "high" },
      ],
    },
  ]);
  expect(roleFallbackChoices(choices, index).map((item) => item.label)).toEqual([
    "native / Human model",
    "native / *",
  ]);
  expect(inheritedRoleFallbacks(feature, index)).toEqual([
    "tokencake-2 / deepseek-v4.1-flash-1017 (auto)",
  ]);
});

it("rejects duplicate roles instead of silently overwriting another row", () => {
  const row = { id: "a", role: "modelRole.advisor", model: "opus", thinking: "" };
  expect(() => encodePresetRoles({ name: "p", model: "m" }, [row, { ...row, id: "b" }])).toThrow();
});

it("shows inherited model with vendor and effort from routing metadata", () => {
  const feature = {
    type: "select" as const,
    id: "modelRole.task",
    label: "Task",
    value: "native",
    options: [
      {
        id: "native",
        label: "Localized default",
        metadata: { nativeSelector: "@smol", resolvedSelector: "provider/model:high" },
      },
    ],
  };
  expect(
    nativeRolePreview(feature, [
      { id: '["provider","model"]', label: "Fast model", provider: "omp" },
    ]),
  ).toEqual({ model: "provider/model", thinking: "high", label: "provider / Fast model" });
  feature.options[0]!.metadata.resolvedSelector = "auto";
  expect(nativeRolePreview(feature, [])).toBeNull();
});

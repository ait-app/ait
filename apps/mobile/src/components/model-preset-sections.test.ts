import { expect, it } from "vitest";
import { presetSectionIsVisible, resolveModelCardSections } from "./model-preset-sections";

it("hides the preset group when the provider has no presets", () => {
  expect(presetSectionIsVisible([])).toBe(false);
});

it("shows the preset group when the provider has presets", () => {
  expect(presetSectionIsVisible([{ id: "p1" }])).toBe(true);
});

it("shows both groups when the provider offers models and has presets", () => {
  expect(resolveModelCardSections({ modelCount: 3, presetCount: 2, modelError: false })).toEqual({
    defaultModel: true,
    modelPresets: true,
  });
});

it("shows only the preset group when presets exist but no model was discovered", () => {
  expect(resolveModelCardSections({ modelCount: 0, presetCount: 1, modelError: false })).toEqual({
    defaultModel: false,
    modelPresets: true,
  });
});

it("withholds the default-model heading when discovery failed", () => {
  // The error state replaces the list, so naming an empty "Default model" group would be a lie.
  expect(resolveModelCardSections({ modelCount: 0, presetCount: 0, modelError: true })).toEqual({
    defaultModel: false,
    modelPresets: false,
  });
});

it("shows no preset group for a provider without presets", () => {
  expect(resolveModelCardSections({ modelCount: 5, presetCount: 0, modelError: false })).toEqual({
    defaultModel: true,
    modelPresets: false,
  });
});

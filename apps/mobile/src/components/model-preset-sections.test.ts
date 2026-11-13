import { expect, it } from "vitest";
import { presetSectionIsVisible } from "./model-preset-sections";

it("hides the preset group when the provider has no presets", () => {
  expect(presetSectionIsVisible([])).toBe(false);
});

it("shows the preset group when the provider has presets", () => {
  expect(presetSectionIsVisible([{ id: "p1" }])).toBe(true);
});

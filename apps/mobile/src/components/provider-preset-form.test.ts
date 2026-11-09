import { expect, it } from "vitest";
import {
  decodePresetRoles,
  nativeRolePreview,
  encodePresetRoles,
  presetModelLabel,
  roleModelChoices,
  splitRoleSelector,
} from "./provider-preset-form";

it("separates native effort suffixes without splitting ordinary model colons", () => {
  expect(splitRoleSelector("gateway:region/model:high")).toEqual({
    model: "gateway:region/model",
    thinking: "high",
  });
  expect(splitRoleSelector("gateway:model-v2")).toEqual({
    model: "gateway:model-v2",
    thinking: "",
  });
});
it("round trips explicit models, role aliases, defaults and unrelated features", () => {
  const preset = {
    name: "work",
    model: "sonnet",
    featureValues: {
      "modelRole.advisor": "provider/model:high",
      "modelRole.task": "@smol",
      "modelRole.memory": "native",
      fastMode: true,
    },
  };
  expect(encodePresetRoles(preset, decodePresetRoles(preset))).toEqual(preset.featureValues);
});
it("removed roles clear native defaults while Ait records omit them", () => {
  const original = {
    name: "work",
    model: "sonnet",
    featureValues: { "modelRole.advisor": "opus", "modelRole.task": "unset" },
  };
  expect(decodePresetRoles(original)).toHaveLength(1);
  expect(encodePresetRoles(original, [])).toEqual({});
  expect(encodePresetRoles(original, [], ["modelRole.advisor", "modelRole.task"])).toEqual({
    "modelRole.advisor": "unset",
    "modelRole.task": "unset",
  });
});
it("does not attach thinking to default or unassigned sentinel values", () => {
  expect(
    encodePresetRoles({ name: "p", model: "m" }, [
      { id: "a", role: "modelRole.advisor", model: "native", thinking: "high" },
    ]),
  ).toEqual({ "modelRole.advisor": "native" });
});
it("groups role-compatible model/effort candidates under human model labels", () => {
  const feature = {
    type: "select" as const,
    id: "modelRole.advisor",
    value: "native",
    label: "Advisor",
    options: [
      { id: "native", label: "Provider default" },
      { id: "unset", label: "Unassigned" },
      { id: "native/model", label: "Raw · native/model" },
      { id: "native/model:high", label: "Raw · native/model (high)" },
    ],
  };
  expect(
    roleModelChoices(feature, [
      { provider: "omp", id: '["native","model"]', label: "Human model" },
    ]),
  ).toEqual([
    { id: "native/model", label: "Human model", thinkingOptions: [{ id: "high", label: "high" }] },
  ]);
  expect(presetModelLabel('["native","other"]', [])).toBe("other");
});
it("rejects duplicate roles instead of silently overwriting another row", () => {
  const row = { id: "a", role: "modelRole.advisor", model: "opus", thinking: "" };
  expect(() => encodePresetRoles({ name: "p", model: "m" }, [row, { ...row, id: "b" }])).toThrow();
});

it("shows inherited model and effort from routing metadata without parsing display labels", () => {
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
  ).toEqual({ model: "provider/model", thinking: "high", label: "Fast model" });
  feature.options[0]!.metadata.resolvedSelector = "auto";
  expect(nativeRolePreview(feature, [])).toBeNull();
});

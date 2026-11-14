import { describe, expect, test } from "vitest";

import { resolveDraftModeId, validateDraftCreation } from "./workspace-tab-core";

const baseComposerState = {
  providerDefinitions: [{ id: "codewhale" }],
  selectedProvider: "codewhale",
  isModelLoading: false,
  effectiveModelId: "",
  availableModels: [],
};

function validate(overrides = {}) {
  return validateDraftCreation({
    composerState: baseComposerState,
    autoSubmitConfig: null,
    workspaceDirectory: "/tmp/project",
    hasClient: true,
    ...overrides,
  });
}

describe("workspace draft agent model validation", () => {
  test("allows a ready provider with no models to submit without a selected model", () => {
    expect(validate({})).toBeNull();
  });

  test("keeps waiting while model defaults are loading", () => {
    expect(
      validate({
        composerState: {
          ...baseComposerState,
          isModelLoading: true,
        },
      }),
    ).toBe("Model defaults are still loading");
  });

  test("still requires a selected model when the provider exposes models", () => {
    expect(
      validate({
        composerState: {
          ...baseComposerState,
          availableModels: [{ id: "deepseek/deepseek-v4-pro" }],
        },
      }),
    ).toBe("No model is available for the selected provider");
  });
});

describe("workspace draft agent mode", () => {
  const modeOptionIds = ["build", "plan"];

  test("uses the auto-submit mode when the setup names one", () => {
    expect(
      resolveDraftModeId({
        autoSubmitConfig: { modeId: "plan" },
        modeOptionIds,
        selectedMode: "build",
      }),
    ).toBe("plan");
  });

  test("falls back to the form's mode when the auto-submit setup names none", () => {
    expect(
      resolveDraftModeId({
        autoSubmitConfig: { modeId: null },
        modeOptionIds,
        selectedMode: "plan",
      }),
    ).toBe("plan");
  });

  test("shows the mode the picker displays when the remembered mode is gone", () => {
    expect(
      resolveDraftModeId({ autoSubmitConfig: null, modeOptionIds, selectedMode: "review" }),
    ).toBe("build");
  });

  test("leaves the mode unset when the provider exposes no modes", () => {
    expect(
      resolveDraftModeId({
        autoSubmitConfig: { modeId: null },
        modeOptionIds: [],
        selectedMode: "",
      }),
    ).toBeNull();
  });
});

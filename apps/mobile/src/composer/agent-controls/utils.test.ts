import { describe, expect, it } from "vitest";
import {
  getRuntimeThinkingState,
  getFeatureHighlightColor,
  getFeatureTooltip,
  getAgentControlHintKey,
  isFeatureActive,
  normalizeModelId,
  resolveAgentModelSelection,
  resolveThinkingDisplay,
} from "./utils";

describe("getAgentControlHintKey", () => {
  it("returns translation keys for each editable agent control hint", () => {
    expect(getAgentControlHintKey("thinking")).toBe("agentControls.hints.thinking");
    expect(getAgentControlHintKey("model")).toBe("agentControls.hints.model");
    expect(getAgentControlHintKey("mode")).toBe("agentControls.hints.mode");
  });
});

describe("feature metadata helpers", () => {
  it("prefers explicit feature tooltip copy", () => {
    expect(
      getFeatureTooltip({
        label: "Plan",
        tooltip: "Toggle plan mode",
      }),
    ).toBe("Toggle plan mode");
  });

  it("falls back to the feature label when no tooltip is provided", () => {
    expect(
      getFeatureTooltip({
        label: "Custom",
      }),
    ).toBe("Custom");
  });

  it("maps feature highlight colors by feature id", () => {
    expect(getFeatureHighlightColor("fast_mode")).toBe("yellow");
    expect(getFeatureHighlightColor("service_tier")).toBe("yellow");
    expect(getFeatureHighlightColor("plan_mode")).toBe("blue");
    expect(getFeatureHighlightColor("other")).toBe("default");
  });

  it("treats a select feature as active when a non-default option is selected", () => {
    const speed = {
      type: "select" as const,
      id: "service_tier",
      label: "Speed",
      options: [
        { id: "default", label: "Normal", isDefault: true },
        { id: "fast", label: "Fast" },
      ],
    };
    expect(isFeatureActive({ ...speed, value: "default" })).toBe(false);
    expect(isFeatureActive({ ...speed, value: "fast" })).toBe(true);
    expect(
      isFeatureActive({
        type: "toggle",
        id: "fast_mode",
        label: "Fast",
        value: true,
      }),
    ).toBe(true);
  });
});

describe("normalizeModelId", () => {
  it("treats empty values as unset", () => {
    expect(normalizeModelId("")).toBeNull();
    expect(normalizeModelId(undefined)).toBeNull();
  });

  it("returns trimmed model ids", () => {
    expect(normalizeModelId(" gpt-5.1-codex ")).toBe("gpt-5.1-codex");
    expect(normalizeModelId(" default ")).toBe("default");
  });
});

describe("resolveAgentModelSelection", () => {
  it("resolves a configured model alias to its canonical catalog model", () => {
    const selection = resolveAgentModelSelection({
      models: [
        {
          provider: "claude",
          id: "claude-fable-5",
          aliases: ["claude-fable-5[1m]"],
          label: "Fable 5",
          thinkingOptions: [{ id: "high", label: "High" }],
          defaultThinkingOptionId: "high",
        },
      ],
      runtimeModelId: null,
      configuredModelId: "claude-fable-5[1m]",
      explicitThinkingOptionId: null,
    });

    expect(selection.activeModelId).toBe("claude-fable-5");
    expect(selection.displayModel).toBe("Fable 5");
    expect(selection.selectedThinkingId).toBe("high");
  });

  it("shows the newly configured model while the previous turn still reports its model", () => {
    const selection = resolveAgentModelSelection({
      models: [
        {
          id: "a",
          provider: "codex",
          label: "Model A",
          thinkingOptions: [{ id: "low", label: "Low" }],
          defaultThinkingOptionId: "low",
        },
        {
          id: "b",
          provider: "codex",
          label: "Model B",
          thinkingOptions: [{ id: "high", label: "High" }],
          defaultThinkingOptionId: "high",
        },
      ],
      runtimeModelId: "a",
      configuredModelId: "b",
      explicitThinkingOptionId: null,
    });

    expect(selection.activeModelId).toBe("b");
    expect(selection.displayModel).toBe("Model B");
    expect(selection.selectedThinkingId).toBe("high");
  });

  it("uses explicit thinking option when provided", () => {
    const selection = resolveAgentModelSelection({
      models: [
        {
          id: "a",
          provider: "codex",
          label: "Model A",
          thinkingOptions: [
            { id: "low", label: "Low" },
            { id: "high", label: "High" },
          ],
          defaultThinkingOptionId: "low",
        },
      ],
      runtimeModelId: "a",
      configuredModelId: null,
      explicitThinkingOptionId: "high",
    });

    expect(selection.selectedThinkingId).toBe("high");
    expect(selection.displayThinking).toBe("High");
  });

  it("formats raw thinking labels in the selected model display", () => {
    const selection = resolveAgentModelSelection({
      models: [
        {
          id: "a",
          provider: "claude",
          label: "Model A",
          thinkingOptions: [
            { id: "none", label: "none" },
            { id: "xhigh", label: "xhigh" },
          ],
        },
      ],
      runtimeModelId: "a",
      configuredModelId: null,
      explicitThinkingOptionId: "xhigh",
    });

    expect(selection.selectedThinkingId).toBe("xhigh");
    expect(selection.displayThinking).toBe("Extra high");
  });

  it("falls back to the provider default model label instead of Auto", () => {
    const selection = resolveAgentModelSelection({
      models: [
        {
          id: "a",
          provider: "codex",
          label: "Model A",
          isDefault: true,
          thinkingOptions: [{ id: "low", label: "Low" }],
          defaultThinkingOptionId: "low",
        },
      ],
      runtimeModelId: null,
      configuredModelId: null,
      explicitThinkingOptionId: null,
    });

    expect(selection.displayModel).toBe("Model A");
    expect(selection.displayThinking).toBe("Low");
  });

  it("prefers the configured model when runtime model is not in the model list", () => {
    const selection = resolveAgentModelSelection({
      models: [
        {
          id: "default",
          provider: "claude",
          label: "Default (Sonnet 4.6)",
          isDefault: true,
          thinkingOptions: [
            { id: "low", label: "Low" },
            { id: "medium", label: "Medium" },
          ],
        },
      ],
      runtimeModelId: "claude-sonnet-4-6-20260101",
      configuredModelId: "default",
      explicitThinkingOptionId: null,
    });

    expect(selection.activeModelId).toBe("default");
    expect(selection.displayModel).toBe("Default (Sonnet 4.6)");
    expect(selection.selectedThinkingId).toBeNull();
    expect(selection.displayThinking).toBe("Default");
  });

  it("uses an option marked as the provider default", () => {
    const selection = resolveAgentModelSelection({
      models: [
        {
          id: "a",
          provider: "opencode",
          label: "Model A",
          thinkingOptions: [
            { id: "off", label: "Off" },
            { id: "low", label: "Low", isDefault: true },
          ],
        },
      ],
      runtimeModelId: "a",
      configuredModelId: null,
      explicitThinkingOptionId: null,
    });

    expect(selection.selectedThinkingId).toBe("low");
    expect(selection.displayThinking).toBe("Low");
  });

  it("preserves explicit thinking IDs absent from the catalog", () => {
    const selection = resolveAgentModelSelection({
      models: [
        {
          id: "a",
          provider: "opencode",
          label: "Model A",
          thinkingOptions: [{ id: "off", label: "Off" }],
        },
      ],
      runtimeModelId: "a",
      configuredModelId: null,
      explicitThinkingOptionId: "future",
    });

    expect(selection.selectedThinkingId).toBe("future");
    expect(selection.displayThinking).toBe("Future");
  });
});

describe("resolveThinkingDisplay", () => {
  it.each([undefined, null, "", " ", "\t\n"])(
    "shows provider inheritance for an unspecified selection (%s)",
    (id) => {
      expect(resolveThinkingDisplay([{ id: "off", label: "Off" }], id)).toBe("Default");
    },
  );

  it("formats an explicit selection", () => {
    expect(resolveThinkingDisplay([{ id: "xhigh", label: "xhigh" }], "xhigh")).toBe("Extra high");
  });

  it.each(["default", " default "])(
    "uses the catalog label when the native option id is default (%s)",
    (id) => {
      expect(resolveThinkingDisplay([{ id: "default", label: "Adaptive effort" }], id)).toBe(
        "Adaptive effort",
      );
    },
  );
});

describe("native thinking readback", () => {
  it.each(["high", "low", "future"])(
    "displays effective %s while preserving the requested selection",
    (level) => {
      const info = {
        provider: "pi",
        sessionId: "session",
        thinkingOptionId: "max",
        extra: { thinkingLevel: level },
      };
      const selection = resolveAgentModelSelection({
        models: [{ id: "a", provider: "pi", label: "Model A" }],
        runtimeModelId: "a",
        configuredModelId: null,
        explicitThinkingOptionId: "max",
        runtimeThinkingState: getRuntimeThinkingState(info),
      });
      expect(selection.selectedThinkingId).toBe("max");
      expect(selection.displayThinkingId).toBe(level);
      expect(selection.displayThinking).toBe(level[0].toUpperCase() + level.slice(1));
    },
  );

  it("shows the native default even when no level was selected or advertised", () => {
    const selection = resolveAgentModelSelection({
      models: null,
      runtimeModelId: "a",
      configuredModelId: null,
      explicitThinkingOptionId: null,
      runtimeThinkingState: { status: "reported", optionId: "medium" },
    });
    expect(selection.selectedThinkingId).toBeNull();
    expect(selection.displayThinkingId).toBe("medium");
    expect(selection.displayThinking).toBe("Medium");
  });

  it("distinguishes missing native state from a reported value", () => {
    for (const extra of [undefined, {}, { thinkingLevel: undefined }]) {
      expect(getRuntimeThinkingState({ provider: "pi", sessionId: "session", extra })).toEqual({
        status: "unreported",
      });
    }
  });

  it.each([null, "", "   ", 42, false, [], {}])(
    "preserves malformed native state (%j)",
    (level) => {
      const runtimeThinkingState = getRuntimeThinkingState({
        provider: "pi",
        sessionId: "session",
        extra: { thinkingLevel: level },
      });
      expect(runtimeThinkingState).toEqual({ status: "malformed" });

      for (const explicitThinkingOptionId of [null, "max"]) {
        for (const defaultThinkingOptionId of [undefined, "low"]) {
          const selection = resolveAgentModelSelection({
            models: [{ id: "a", provider: "pi", label: "Model A", defaultThinkingOptionId }],
            runtimeModelId: "a",
            configuredModelId: null,
            explicitThinkingOptionId,
            runtimeThinkingState,
          });
          expect(selection.selectedThinkingId).toBe(
            explicitThinkingOptionId ?? defaultThinkingOptionId ?? null,
          );
          expect(selection.displayThinkingId).toBeNull();
          expect(selection.isThinkingUnknown).toBe(true);
          expect(selection.displayThinking).toBe("Unknown");
        }
      }
    },
  );

  it("uses the requested selection or default only when native state is unreported", () => {
    for (const explicitThinkingOptionId of [null, "max"]) {
      const selection = resolveAgentModelSelection({
        models: null,
        runtimeModelId: "a",
        configuredModelId: null,
        explicitThinkingOptionId,
        runtimeThinkingState: { status: "unreported" },
      });
      expect(selection.isThinkingUnknown).toBe(false);
      expect(selection.displayThinking).toBe(explicitThinkingOptionId ? "Max" : "Default");
    }
  });

  it("does not interpret another provider's metadata as Pi state", () => {
    expect(
      getRuntimeThinkingState({
        provider: "claude",
        sessionId: "session",
        thinkingOptionId: "max",
        extra: { thinkingLevel: "high" },
      }),
    ).toEqual({ status: "unreported" });
    expect(getRuntimeThinkingState(null)).toEqual({ status: "unreported" });
    expect(getRuntimeThinkingState(undefined)).toEqual({ status: "unreported" });
  });
});

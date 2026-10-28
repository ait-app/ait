import { describe, expect, it } from "vitest";
import type { AgentModelDefinition, AgentRuntimeInfo } from "@ait/protocol/agent-types";
import {
  getRuntimeThinkingState,
  resolveDefaultThinkingOptionId,
  resolveProviderControlPresentation,
} from "./control-presentation";

const runtime: AgentRuntimeInfo = {
  provider: "pi",
  sessionId: "session",
  thinkingOptionId: "future",
  extra: { thinkingLevel: "low", configPending: true },
};

describe("provider control presentation boundary", () => {
  it.each(["codex", "claude", "opencode", "deepseek-harness", "omp", "future"])(
    "keeps %s controls independent of native extras",
    (provider) => {
      expect(resolveProviderControlPresentation(provider, { ...runtime, provider })).toEqual({
        preferRuntimeModel: false,
        selectedThinkingId: null,
        runtimeThinkingStatus: "unreported",
        runtimeThinkingOptionId: null,
        preserveUnknownThinking: false,
      });
    },
  );

  it("keeps queued selection separate from effective native facts", () => {
    expect(resolveProviderControlPresentation("pi", runtime)).toEqual({
      preferRuntimeModel: false,
      selectedThinkingId: null,
      runtimeThinkingStatus: "reported",
      runtimeThinkingOptionId: "future",
      preserveUnknownThinking: true,
    });
  });

  it("normalizes canonical and compatibility readback for every consumer", () => {
    const legacy = { ...runtime, thinkingOptionId: undefined, extra: { thinkingLevel: "xhigh" } };
    expect(resolveProviderControlPresentation(undefined, legacy)).toMatchObject({
      preferRuntimeModel: true,
      selectedThinkingId: "xhigh",
      runtimeThinkingStatus: "reported",
      runtimeThinkingOptionId: "xhigh",
    });
    expect(
      resolveProviderControlPresentation("pi", { ...runtime, extra: {} }).selectedThinkingId,
    ).toBe("future");
  });

  it("uses the explicit provider when runtime identity differs", () => {
    expect(resolveProviderControlPresentation("claude", runtime).preserveUnknownThinking).toBe(
      false,
    );
    expect(getRuntimeThinkingState(runtime, "claude")).toEqual({ status: "unreported" });
  });

  it.each(["pi", "codex", "claude", "opencode", "deepseek-harness", "omp"])(
    "honors advertised defaults for %s",
    (provider) => {
      const model: AgentModelDefinition = {
        provider,
        id: "model",
        label: "Model",
        thinkingOptions: [
          { id: "low", label: "Low" },
          { id: "high", label: "High", isDefault: true },
        ],
      };
      expect(resolveDefaultThinkingOptionId(model)).toBe("high");
      expect(resolveDefaultThinkingOptionId({ ...model, defaultThinkingOptionId: "low" })).toBe(
        "low",
      );
      const unspecified = { ...model, thinkingOptions: [{ id: "low", label: "Low" }] };
      expect(resolveDefaultThinkingOptionId(unspecified)).toBe(provider === "pi" ? "" : "low");
    },
  );

  it("preserves unspecified selections without a model or choices", () => {
    expect(resolveDefaultThinkingOptionId(null)).toBe("");
    expect(resolveDefaultThinkingOptionId({ provider: "pi", id: "model", label: "Model" })).toBe(
      "",
    );
    expect(resolveProviderControlPresentation()).toMatchObject({ preferRuntimeModel: false });
  });
});

/** @vitest-environment jsdom */
import { renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { AgentRuntimeInfo } from "@ait/protocol/agent-types";
import { buildAgentFactsLine, useAgentFactsLine, type AgentFactsInput } from "./agent-facts-line";

const { current } = vi.hoisted(() => ({
  current: {
    provider: "pi",
    cwd: "/project",
    model: "backend/model",
    thinkingOptionId: "max",
    runtimeInfo: {
      provider: "pi",
      sessionId: "session",
      model: "backend/model",
      thinkingOptionId: "max",
      extra: { thinkingLevel: "high" },
    } as AgentRuntimeInfo,
    availableModes: [],
  },
}));

vi.mock("@/hooks/use-providers-snapshot", () => ({
  useProvidersSnapshot: () => ({ entries: [] }),
}));
vi.mock("@/stores/session-store", () => ({
  useSessionStore: (select: (state: unknown) => unknown) =>
    select({
      sessions: { server: { agents: new Map([["agent", current]]) } },
    }),
}));

const input: AgentFactsInput = {
  models: null,
  runtimeModelId: "backend/model",
  configuredModelId: null,
  explicitThinkingOptionId: "max",
  runtimeThinkingState: { status: "reported", optionId: "high" },
  availableModes: [],
  currentModeId: null,
  contextWindowUsedTokens: null,
  contextWindowMaxTokens: null,
};

describe("agent thinking facts", () => {
  it("shows the effective native level for clamped selections and provider defaults", () => {
    expect(buildAgentFactsLine(input)).toBe("backend/model · High");
    expect(buildAgentFactsLine({ ...input, explicitThinkingOptionId: null })).toBe(
      "backend/model · High",
    );
  });

  it("falls back to the requested selection when native state is missing", () => {
    expect(buildAgentFactsLine({ ...input, runtimeThinkingState: { status: "unreported" } })).toBe(
      "backend/model · Max",
    );
  });

  it.each([null, "max"])(
    "shows unknown for malformed readback with requested selection %s",
    (explicitThinkingOptionId) => {
      expect(
        buildAgentFactsLine({
          ...input,
          explicitThinkingOptionId,
          runtimeThinkingState: { status: "malformed" },
        }),
      ).toBe("backend/model · Unknown");
    },
  );

  it("reads runtime metadata from snapshots and follows extension changes", () => {
    const view = renderHook(() => useAgentFactsLine("server", "agent"));
    expect(view.result.current).toBe("backend/model · High");
    current.runtimeInfo = { ...current.runtimeInfo, extra: { thinkingLevel: "low" } };
    view.rerender();
    expect(view.result.current).toBe("backend/model · Low");
    expect(current.thinkingOptionId).toBe("max");
    current.runtimeInfo = { ...current.runtimeInfo, extra: { thinkingLevel: 42 } };
    view.rerender();
    expect(view.result.current).toBe("backend/model · Unknown");
    current.runtimeInfo = { ...current.runtimeInfo, extra: {} };
    view.rerender();
    expect(view.result.current).toBe("backend/model · Max");
  });
});

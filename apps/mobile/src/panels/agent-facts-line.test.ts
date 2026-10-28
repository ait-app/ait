/** @vitest-environment jsdom */
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { AgentRuntimeInfo } from "@ait/protocol/agent-types";
import { buildAgentFactsLine, useAgentFactsLine, type AgentFactsInput } from "./agent-facts-line";
import { getRuntimeThinkingState } from "@/provider-selection/control-presentation";

const { current, publishSnapshot } = vi.hoisted(() => ({
  publishSnapshot: vi.fn(),
  current: {
    provider: "pi",
    cwd: "/project",
    model: "backend/model",
    thinkingOptionId: "max",
    runtimeInfo: {
      provider: "pi",
      sessionId: "session",
      model: "backend/model",
      thinkingOptionId: "high",
      extra: { thinkingLevel: "max" },
    } as AgentRuntimeInfo,
    availableModes: [],
  },
}));

vi.mock("@/hooks/use-providers-snapshot", () => ({
  useProvidersSnapshot: () => ({
    entries: [
      {
        provider: "pi",
        models: [
          {
            provider: "pi",
            id: "backend/model",
            label: "backend/model",
            thinkingOptions: [{ id: "max", label: "Max" }],
          },
        ],
      },
    ],
  }),
}));
vi.mock("@/stores/session-store", async () => {
  const { create } = await import("zustand");
  const snapshot = () => ({ sessions: { server: { agents: new Map([["agent", current]]) } } });
  const store = create(() => snapshot());
  publishSnapshot.mockImplementation(() => store.setState(snapshot()));
  return { useSessionStore: store };
});

const input: AgentFactsInput = {
  provider: "pi",
  runtimeInfo: null,
  models: [
    {
      provider: "pi",
      id: "backend/model",
      label: "backend/model",
      thinkingOptions: [{ id: "max", label: "Max" }],
    },
  ],
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
  it("skips equivalent runtime snapshots and follows changed native thinking", () => {
    const previous = current.runtimeInfo;
    const render = vi.fn(() => useAgentFactsLine("server", "agent"));
    const view = renderHook(render);
    const renders = render.mock.calls.length;
    act(() => {
      current.runtimeInfo = { ...previous, extra: { ...previous.extra } };
      publishSnapshot();
    });
    expect(render).toHaveBeenCalledTimes(renders);
    act(() => {
      current.runtimeInfo = { ...previous, thinkingOptionId: "low" };
      publishSnapshot();
    });
    expect(render.mock.calls.length).toBeGreaterThan(renders);
    expect(view.result.current).toBe("backend/model · Low");
    view.unmount();
    current.runtimeInfo = previous;
  });

  it("prefers Pi's native model over a different saved model", () => {
    expect(
      buildAgentFactsLine({
        ...input,
        configuredModelId: "backend/saved",
        models: [...input.models!, { provider: "pi", id: "backend/saved", label: "Saved" }],
        runtimeInfo: { provider: "pi", sessionId: "s", model: "backend/model" },
      }),
    ).toBe("backend/model · High");
  });

  it("labels future native levels even without a catalog entry", () => {
    expect(
      buildAgentFactsLine({
        ...input,
        models: null,
        runtimeThinkingState: {
          status: "reported",
          optionId: "future",
        },
      }),
    ).toBe("backend/model · Future");
  });

  it("preserves the explicit selection when Pi does not report a level", () => {
    expect(
      buildAgentFactsLine({
        ...input,
        explicitThinkingOptionId: "future",
        runtimeThinkingState: { status: "unreported" },
      }),
    ).toBe("backend/model · Future");
    expect(
      buildAgentFactsLine({
        ...input,
        models: null,
        runtimeThinkingState: { status: "unreported" },
      }),
    ).toBe("backend/model · Max");
  });

  it("shows the effective native level for clamped selections and provider defaults", () => {
    expect(buildAgentFactsLine(input)).toBe("backend/model · High");
    expect(buildAgentFactsLine({ ...input, explicitThinkingOptionId: null })).toBe(
      "backend/model · High",
    );
  });

  it("falls back to the requested selection when native state is missing", () => {
    expect(
      buildAgentFactsLine({
        ...input,
        runtimeThinkingState: { status: "unreported" },
      }),
    ).toBe("backend/model · Max");
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
    current.runtimeInfo = {
      ...current.runtimeInfo,
      thinkingOptionId: "low",
    };
    view.rerender();
    expect(view.result.current).toBe("backend/model · Low");
    expect(current.thinkingOptionId).toBe("max");
    current.runtimeInfo = {
      ...current.runtimeInfo,
      thinkingOptionId: undefined,
      extra: { thinkingLevel: 42 },
    };
    view.rerender();
    expect(view.result.current).toBe("backend/model · Unknown");
    current.runtimeInfo = { ...current.runtimeInfo, extra: {} };
    view.rerender();
    expect(view.result.current).toBe("backend/model · Max");
  });
});

describe("Pi runtime thinking metadata", () => {
  it("does not project foreign provider metadata into another provider's facts", () => {
    const previousProvider = current.provider;
    const previousInfo = current.runtimeInfo;
    current.provider = "claude";
    current.runtimeInfo = { ...previousInfo, thinkingOptionId: "low" };
    try {
      const view = renderHook(() => useAgentFactsLine("server", "agent"));
      expect(view.result.current).toBe("backend/model");
    } finally {
      current.provider = previousProvider;
      current.runtimeInfo = previousInfo;
    }
  });
  it("prefers canonical thinking over compatibility metadata", () => {
    expect(
      getRuntimeThinkingState({
        provider: "pi",
        sessionId: "s",
        thinkingOptionId: "low",
        extra: { thinkingLevel: "high" },
      }),
    ).toEqual({ status: "reported", optionId: "low" });
  });
  it.each(["high", "ultra"])("reads canonical thinking without extra metadata: %s", (level) => {
    expect(
      getRuntimeThinkingState({
        provider: "pi",
        sessionId: "s",
        thinkingOptionId: level,
      }),
    ).toEqual({ status: "reported", optionId: level });
  });
  it.each([null, "", "   ", 42, false, [], {}])("recognizes malformed state %j", (level) => {
    expect(
      getRuntimeThinkingState({
        provider: "pi",
        sessionId: "s",
        extra: { thinkingLevel: level },
      }),
    ).toEqual({ status: "malformed" });
  });
  it.each(["high", "low", "future"])("preserves native level %s", (level) => {
    expect(
      getRuntimeThinkingState({
        provider: "pi",
        sessionId: "s",
        extra: { thinkingLevel: level },
      }),
    ).toEqual({ status: "reported", optionId: level });
  });
  it("ignores missing metadata and other providers", () => {
    for (const info of [
      null,
      undefined,
      { provider: "pi", sessionId: "s" },
      {
        provider: "claude",
        sessionId: "s",
        extra: { thinkingLevel: "high" },
      },
    ]) {
      expect(getRuntimeThinkingState(info)).toEqual({
        status: "unreported",
      });
    }
  });
});

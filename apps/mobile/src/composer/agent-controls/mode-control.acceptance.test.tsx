/** @vitest-environment jsdom */
import React from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { useLiveAgentModeControl } from "./mode-control";

const fixture = vi.hoisted(() => {
  const agent = {
    provider: "omp",
    cwd: "/repo",
    currentModeId: "ask",
    availableModes: [
      { id: "ask", label: "Ask" },
      { id: "full", label: "Full" },
    ],
  };
  const setAgentMode = vi.fn();
  const updatePreferences = vi.fn();
  const toast = { error: vi.fn(), show: vi.fn() };
  return {
    state: {
      sessions: { host: { agents: new Map([["agent", agent]]), client: { setAgentMode } } },
    },
    setAgentMode,
    updatePreferences,
    toast,
  };
});
vi.mock("@/stores/session-store", () => ({
  useSessionStore: (select: (state: unknown) => unknown) => select(fixture.state),
}));
vi.mock("zustand/traditional", () => ({
  useStoreWithEqualityFn: (_store: unknown, select: (state: unknown) => unknown) =>
    select(fixture.state),
}));
vi.mock("@/hooks/use-providers-snapshot", () => ({
  useProvidersSnapshot: () => ({ entries: [] }),
}));
vi.mock("@/hooks/use-form-preferences", async (original) => ({
  ...(await original<typeof import("@/hooks/use-form-preferences")>()),
  useFormPreferences: () => ({ updatePreferences: fixture.updatePreferences }),
}));
vi.mock("@/contexts/toast-context", () => ({ useToast: () => fixture.toast }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("@/i18n/i18next", () => ({ i18n: { t: (key: string) => key } }));
const gate = { busy: false, run: async (task: () => Promise<void>) => task() };
beforeEach(() => {
  fixture.setAgentMode.mockReset();
  fixture.updatePreferences.mockReset();
  fixture.toast.error.mockReset();
});

it("does not update the saved mode before the host accepts it", async () => {
  let accept!: (notice: null) => void;
  fixture.setAgentMode.mockReturnValue(
    new Promise((resolve) => {
      accept = resolve;
    }),
  );
  const { result } = renderHook(() => useLiveAgentModeControl("host", "agent", gate));
  act(() => result.current!.onSelectMode("full"));
  expect(fixture.updatePreferences).not.toHaveBeenCalled();
  await act(async () => accept(null));
  await waitFor(() => expect(fixture.updatePreferences).toHaveBeenCalledOnce());
});

it("keeps saved mode unchanged when the host rejects the selection", async () => {
  fixture.setAgentMode.mockRejectedValue(new Error("rejected"));
  const { result } = renderHook(() => useLiveAgentModeControl("host", "agent", gate));
  act(() => result.current!.onSelectMode("full"));
  await waitFor(() => expect(fixture.toast.error).toHaveBeenCalledWith("rejected"));
  expect(fixture.updatePreferences).not.toHaveBeenCalled();
});

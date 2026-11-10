/** @vitest-environment jsdom */
import React from "react";
import { act, cleanup, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { AgentControlCommandCenterSource } from "@/command-center/agent-control-registration";
import { AgentControls } from "./index";

const fixture = vi.hoisted(() => ({
  feature: vi.fn().mockResolvedValue({}),
  apply: vi.fn().mockResolvedValue({}),
  preferences: vi.fn().mockResolvedValue({}),
  controls: null as AgentControlCommandCenterSource | null,
  native: false,
  actions: false,
}));
vi.mock("@/stores/session-store", () => ({
  useSessionStore: Object.assign(
    (select: (state: unknown) => unknown) =>
      select({
        sessions: {
          host: {
            client: { setAgentFeature: fixture.feature, applyAgentConfig: fixture.apply },
            agents: new Map([
              [
                "agent",
                {
                  provider: fixture.native ? "omp" : "claude",
                  cwd: "/project",
                  workspaceId: "workspace",
                  providerUi: fixture.actions
                    ? {
                        actions: [
                          { id: "get_session_stats", label: "Session statistics", fields: [] },
                        ],
                      }
                    : undefined,
                  model: "opus",
                  features: [
                    {
                      type: "select",
                      id: "modelPreset",
                      label: "Preset",
                      value: "__custom__",
                      options: [
                        { id: "__custom__", label: "Custom" },
                        {
                          id: "work",
                          label: "Work",
                          metadata: {
                            source: fixture.native ? "global" : "ait",
                            configuration: { modelId: "opus" },
                          },
                        },
                      ],
                    },
                  ],
                },
              ],
            ]),
          },
        },
      }),
    { getState: () => ({ sessions: {} }) },
  ),
}));
vi.mock("@/components/provider-native-actions", () => ({
  ProviderNativeActions: ({
    serverId,
    agentId,
    cwd,
    workspaceId,
    disabled,
  }: {
    serverId: string;
    agentId: string;
    cwd: string;
    workspaceId: string;
    disabled: boolean;
  }) => (
    <button
      data-testid="toolbar-actions"
      data-target={`${serverId}/${agentId}/${workspaceId}`}
      data-cwd={cwd}
      disabled={disabled}
    >
      …
    </button>
  ),
}));
vi.mock("./model-sheet", () => ({ CompactModelSheet: () => null }));
vi.mock("@/components/ui/combobox", () => ({ Combobox: () => null, ComboboxItem: () => null }));
vi.mock("@/hooks/use-providers-snapshot", () => ({
  useProvidersSnapshot: () => ({ entries: [], refetchIfStale: vi.fn(), refresh: vi.fn() }),
}));
vi.mock("@/hooks/use-form-preferences", () => ({
  useFormPreferences: () => ({ updatePreferences: fixture.preferences }),
  mergeProviderPreferences: () => ({}),
}));
vi.mock("@/contexts/toast-context", () => ({ useToast: () => ({ error: vi.fn() }) }));
vi.mock("@/agent-profiles", () => ({
  useAgentProfilePicker: () => null,
  useAgentProfileEditor: () => ({ element: null }),
}));
vi.mock("./mode-control", () => ({
  useLiveAgentModeControl: () => null,
  AgentModeControl: () => null,
}));
vi.mock("@/composer/keyboard-scope", () => ({
  useComposerKeyboardScope: () => ({ isActiveComposer: true }),
}));
vi.mock("@/command-center/agent-control-registration", () => ({
  useAgentControlCommandCenterActions: ({
    controls,
  }: {
    controls: AgentControlCommandCenterSource;
  }) => {
    fixture.controls = controls;
  },
}));
vi.mock("@/hooks/use-shortcut-keys", () => ({ useShortcutKeys: () => [] }));
vi.mock("@/hooks/use-keyboard-action-handler", () => ({
  useKeyboardActionHandler: () => undefined,
}));
vi.mock("@/components/combined-model-selector", () => ({ CombinedModelSelector: () => null }));

beforeEach(() => {
  vi.stubGlobal("React", React);
  vi.clearAllMocks();
  fixture.native = false;
  fixture.actions = false;
});
afterEach(cleanup);

it.each([false, true])(
  "live session custom selection respects native ownership = %s",
  async (native) => {
    fixture.native = native;
    const queries = new QueryClient();
    render(
      <QueryClientProvider client={queries}>
        <AgentControls serverId="host" agentId="agent" isCompactLayout={false} />
      </QueryClientProvider>,
    );
    await act(async () => {
      await fixture.controls!.features!.set!("modelPreset", "__custom__");
    });
    if (native) {
      expect(fixture.feature).toHaveBeenCalledExactlyOnceWith("agent", "modelPreset", "__custom__");
      expect(fixture.preferences).toHaveBeenCalledOnce();
    } else {
      expect(fixture.feature).not.toHaveBeenCalled();
      expect(fixture.preferences).not.toHaveBeenCalled();
    }
    expect(fixture.apply).not.toHaveBeenCalled();
    queries.clear();
  },
);

it("renders advertised actions in the live composer toolbar and retains blocking", () => {
  const queries = new QueryClient();
  const props = { serverId: "host", agentId: "agent", isCompactLayout: false };
  const view = render(
    <QueryClientProvider client={queries}>
      <AgentControls {...props} />
    </QueryClientProvider>,
  );
  expect(screen.queryByTestId("toolbar-actions")).toBeNull();
  fixture.actions = true;
  view.rerender(
    <QueryClientProvider client={queries}>
      <AgentControls {...props} nativeActionsBlocked />
    </QueryClientProvider>,
  );
  const control = screen.getByTestId("toolbar-actions") as HTMLButtonElement;
  expect(control.dataset.target).toBe("host/agent/workspace");
  expect(control.dataset.cwd).toBe("/project");
  expect(control.disabled).toBe(true);
  view.rerender(
    <QueryClientProvider client={queries}>
      <AgentControls {...props} isCompactLayout nativeActionsBlocked={false} />
    </QueryClientProvider>,
  );
  expect((screen.getByTestId("toolbar-actions") as HTMLButtonElement).disabled).toBe(false);
  queries.clear();
});

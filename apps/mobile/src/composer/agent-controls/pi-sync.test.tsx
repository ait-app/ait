/** @vitest-environment jsdom */
import React, { type ReactNode } from "react";
import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { AgentControls } from "./index";

const fixture = vi.hoisted(() => ({
  setAgentModel: vi.fn(),
  publishSnapshot: vi.fn(),
  modelRenders: vi.fn(),
  agent: {
    provider: "pi",
    cwd: "/project",
    model: "backend/model",
    thinkingOptionId: "high",
    runtimeInfo: {
      provider: "pi",
      sessionId: "session",
      model: "backend/model",
      thinkingOptionId: "minimal",
      extra: { thinkingLevel: "minimal" },
    },
  },
  models: [
    {
      provider: "pi",
      id: "backend/other",
      label: "Other model",
      thinkingOptions: [{ id: "high", label: "High" }],
    },
    {
      provider: "pi",
      id: "backend/model",
      label: "Model",
      thinkingOptions: [
        { id: "off", label: "Off" },
        { id: "minimal", label: "Minimal" },
        { id: "high", label: "High" },
      ],
    },
  ],
}));
vi.mock("@/stores/session-store", async () => {
  const { create } = await import("zustand");
  const client = { setAgentModel: fixture.setAgentModel };
  const snapshot = () => ({
    sessions: {
      server: {
        agents: new Map([["agent", { ...fixture.agent }]]),
        client,
      },
    },
  });
  const store = create(() => snapshot());
  fixture.publishSnapshot.mockImplementation(() => store.setState(snapshot()));
  return { useSessionStore: store };
});
vi.mock("@/agent-profiles", () => ({
  useAgentProfilePicker: () => null,
  useAgentProfileEditor: () => ({ element: null }),
}));
vi.mock("./mode-control", async (original) => ({
  ...(await original<typeof import("./mode-control")>()),
  useLiveAgentModeControl: () => null,
}));
vi.mock("./model-sheet", () => ({
  CompactModelSheet: ({ children, ...props }: ModelSelectorProps & { children: ReactNode }) => (
    <>
      <TestModelSelector {...props} />
      {children}
    </>
  ),
}));
vi.mock("@/components/combined-model-selector", () => ({
  CombinedModelSelector: (props: ModelSelectorProps) => <TestModelSelector {...props} />,
}));
vi.mock("@/components/ui/combobox", () => ({
  Combobox: ({ value, children }: { value: string; children: ReactNode }) => (
    <div data-testid="thinking-selection" data-value={value}>
      {children}
    </div>
  ),
  ComboboxItem: () => null,
}));
vi.mock("@/hooks/use-shortcut-keys", () => ({ useShortcutKeys: () => [] }));
vi.mock("@/hooks/use-keyboard-action-handler", () => ({
  useKeyboardActionHandler: () => undefined,
}));
vi.mock("@/hooks/use-providers-snapshot", () => ({
  useProvidersSnapshot: () => ({
    entries: [{ provider: "pi", status: "ready", enabled: true, models: fixture.models }],
  }),
}));
vi.mock("@/hooks/use-form-preferences", () => ({
  useFormPreferences: () => ({ preferences: {}, updatePreferences: vi.fn() }),
}));
vi.mock("@/command-center/agent-control-registration", () => ({
  useAgentControlCommandCenterActions: () => undefined,
}));
vi.mock("@/contexts/toast-context", () => ({ useToast: () => ({ error: vi.fn() }) }));

type ModelSelectorProps = {
  selectedModel: string;
  onSelect: (provider: string, model: string) => void;
};

function TestModelSelector({ selectedModel, onSelect }: ModelSelectorProps) {
  fixture.modelRenders();
  return (
    <button
      data-testid="model-selection"
      data-value={selectedModel}
      onClick={() => onSelect("pi", "backend/other")}
    >
      Select other model
    </button>
  );
}

beforeEach(async () => {
  vi.stubGlobal("React", React);
  fixture.agent.model = "backend/model";
  fixture.agent.runtimeInfo = {
    ...fixture.agent.runtimeInfo,
    model: "backend/model",
    thinkingOptionId: "minimal",
    extra: { thinkingLevel: "minimal" },
  };
  fixture.setAgentModel.mockReset();
  fixture.modelRenders.mockClear();
  fixture.publishSnapshot();
  fixture.setAgentModel.mockImplementation(async (_agentId: string, model: string) => {
    fixture.agent.model = model;
    fixture.agent.thinkingOptionId = "high";
    fixture.agent.runtimeInfo = {
      ...fixture.agent.runtimeInfo,
      model,
      thinkingOptionId: "high",
      extra: { thinkingLevel: "high" },
    };
    fixture.publishSnapshot();
    return undefined;
  });
  await i18n.changeLanguage("en");
});
afterEach(cleanup);

it("skips equivalent runtime snapshots and still follows native selection changes", () => {
  const view = render(<AgentControls serverId="server" agentId="agent" isCompactLayout={false} />);
  const renders = fixture.modelRenders.mock.calls.length;
  act(() => {
    fixture.agent.runtimeInfo = {
      ...fixture.agent.runtimeInfo,
      extra: { ...fixture.agent.runtimeInfo.extra },
    };
    fixture.publishSnapshot();
  });
  expect(fixture.modelRenders).toHaveBeenCalledTimes(renders);
  act(() => {
    fixture.agent.runtimeInfo = {
      ...fixture.agent.runtimeInfo,
      thinkingOptionId: "high",
      extra: { thinkingLevel: "high" },
    };
    fixture.publishSnapshot();
  });
  expect(fixture.modelRenders.mock.calls.length).toBeGreaterThan(renders);
  expect(view.getByRole("button", { name: "Select thinking option (High)" })).toBeTruthy();
});

it("reloads and updates the composer label and checkmark from Pi instead of the saved request", () => {
  const view = render(<AgentControls serverId="server" agentId="agent" isCompactLayout={false} />);
  expect(view.getByRole("button", { name: "Select thinking option (Minimal)" })).toBeTruthy();
  expect(view.getByTestId("thinking-selection").getAttribute("data-value")).toBe("minimal");
  fixture.agent.runtimeInfo = {
    ...fixture.agent.runtimeInfo,
    thinkingOptionId: "high",
    extra: { thinkingLevel: "high" },
  };
  act(() => fixture.publishSnapshot());
  view.rerender(
    <AgentControls
      serverId="server"
      agentId="agent"
      isCompactLayout={false}
      onDropdownClose={() => {}}
    />,
  );
  expect(view.getByRole("button", { name: "Select thinking option (High)" })).toBeTruthy();
  fixture.agent.runtimeInfo = {
    ...fixture.agent.runtimeInfo,
    thinkingOptionId: "future",
    extra: { thinkingLevel: "future" },
  };
  act(() => fixture.publishSnapshot());
  view.rerender(
    <AgentControls
      serverId="server"
      agentId="agent"
      isCompactLayout={false}
      onDropdownClose={() => {}}
    />,
  );
  expect(view.getByRole("button", { name: "Select thinking option (Future)" })).toBeTruthy();
  expect(view.getByTestId("thinking-selection").getAttribute("data-value")).toBe("future");
});

it.each([false, true])(
  "selects another model within Pi and follows its native readback (compact=%s)",
  async (isCompactLayout) => {
    const props = { serverId: "server", agentId: "agent", isCompactLayout };
    const view = render(<AgentControls {...props} />);
    expect(view.getByTestId("model-selection").getAttribute("data-value")).toBe("backend/model");
    await act(async () => {
      fireEvent.click(view.getByRole("button", { name: "Select other model" }));
    });
    expect(fixture.setAgentModel).toHaveBeenCalledWith("agent", "backend/other");
    view.rerender(<AgentControls {...props} onDropdownClose={() => {}} />);
    expect(view.getByTestId("model-selection").getAttribute("data-value")).toBe("backend/other");
    expect(view.getByRole("button", { name: "Select thinking option (High)" })).toBeTruthy();
    expect(view.getByTestId("thinking-selection").getAttribute("data-value")).toBe("high");

    fixture.agent.runtimeInfo = {
      ...fixture.agent.runtimeInfo,
      model: "backend/other",
      thinkingOptionId: "minimal",
      extra: { thinkingLevel: "minimal" },
    };
    act(() => fixture.publishSnapshot());
    view.rerender(<AgentControls {...props} onDropdownClose={() => {}} />);
    expect(view.getByTestId("model-selection").getAttribute("data-value")).toBe("backend/other");
    expect(view.getByRole("button", { name: "Select thinking option (Minimal)" })).toBeTruthy();
    expect(view.getByTestId("thinking-selection").getAttribute("data-value")).toBe("minimal");
  },
);

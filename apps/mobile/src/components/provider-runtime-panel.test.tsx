// @vitest-environment jsdom
import React from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ProviderUiState } from "@ait/protocol/messages";
import { useBtwVisibilityStore } from "@/stores/btw-visibility-store";
import { ProviderRuntimePanel } from "./provider-runtime-panel";

const fixture = vi.hoisted(() => ({
  state: null as ProviderUiState | null,
  mainTurnActive: false,
  run: vi.fn(async () => ({ text: "Done" })),
  open: vi.fn(async () => {}),
}));
vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeClient: () => ({ runNativeAction: fixture.run }),
}));
vi.mock("@/stores/session-store", () => {
  const getState = () => ({
    sessions: {
      server: {
        agents: new Map([
          [
            "agent",
            {
              providerUi: fixture.state,
              turn: { phase: fixture.mainTurnActive ? "open" : "idle" },
            },
          ],
        ]),
      },
    },
  });
  return {
    useSessionStore: Object.assign(
      (selector: (state: unknown) => unknown) => selector(getState()),
      { getState },
    ),
  };
});
vi.mock("@/hooks/use-file-download", () => ({ useFileDownload: () => vi.fn() }));
vi.mock("@/utils/open-external-url", () => ({ openExternalUrl: fixture.open }));
vi.mock("./provider-side-questions", () => ({
  ProviderSideQuestions: ({ onEnd }: { onEnd: () => void }) => (
    <div data-testid="mock-side-question-panel">
      <button onClick={onEnd}>End BTW</button>
    </div>
  ),
}));
vi.mock("@/hooks/use-settings", () => ({
  useSettings: (selector: (state: unknown) => unknown) =>
    selector({ openInSidePane: { subagents: false } }),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => (key === "providerControl.openBrowser" ? "Open in browser" : key),
  }),
}));
beforeEach(async () => {
  await useBtwVisibilityStore.persist.rehydrate();
  useBtwVisibilityStore.setState({ ended: [], hydrated: true });
});
afterEach(() => {
  cleanup();
  fixture.open.mockClear();
  fixture.run.mockClear();
  fixture.mainTurnActive = false;
});

it("updates and removes native UI, preserves widget placement, and opens links on click", () => {
  fixture.state = {
    statuses: [{ key: "build", text: "Build ready" }],
    widgets: [{ key: "summary", lines: ["First", "Second"], placement: "belowEditor" }],
    title: "Native session",
    editor: null,
    link: {
      url: "https://example.com/auth",
      launchUrl: "https://example.com/launch",
      instructions: "Enter code",
    },
  };
  const above = render(
    <ProviderRuntimePanel serverId="server" agentId="agent" placement="aboveEditor" />,
  );
  expect(screen.getByRole("button", { name: "Build ready" })).toBeTruthy();
  expect(screen.queryByText(/First/)).toBeNull();
  expect(fixture.open).not.toHaveBeenCalled();
  fireEvent.click(screen.getByText("Open in browser"));
  expect(fixture.open).toHaveBeenCalledExactlyOnceWith("https://example.com/launch");
  render(<ProviderRuntimePanel serverId="server" agentId="agent" placement="belowEditor" />);
  fireEvent.click(screen.getByTestId("provider-control-toggle-widget:summary"));
  expect(screen.getByText(/First.*Second/)).toBeTruthy();
  fixture.state = { ...fixture.state, statuses: [], widgets: [], link: null, title: null };
  above.rerender(
    <ProviderRuntimePanel serverId="server" agentId="agent" placement="aboveEditor" />,
  );
  expect(screen.queryByText("Build ready")).toBeNull();
  expect(screen.queryByText("Open in browser")).toBeNull();
});

it("defers editor replacement during submit and delivers each revision once across navigation", () => {
  fixture.state = {
    statuses: [],
    widgets: [],
    title: null,
    link: null,
    editor: { id: "revision-one", text: "Native draft" },
  };
  const onEditor = vi.fn();
  const props = {
    serverId: "server",
    agentId: "agent",
    placement: "aboveEditor" as const,
    onEditor,
  };
  const mounted = render(<ProviderRuntimePanel {...props} blocked />);
  expect(onEditor).not.toHaveBeenCalled();
  mounted.rerender(<ProviderRuntimePanel {...props} />);
  expect(onEditor).toHaveBeenCalledExactlyOnceWith("Native draft");
  mounted.unmount();
  const reopened = render(<ProviderRuntimePanel {...props} />);
  expect(onEditor).toHaveBeenCalledTimes(1);
  fixture.state = { ...fixture.state, editor: { id: "revision-two", text: "" } };
  reopened.rerender(<ProviderRuntimePanel {...props} />);
  expect(onEditor).toHaveBeenLastCalledWith("");
  expect(onEditor).toHaveBeenCalledTimes(2);
});

it("shows one Vibe status in the control row and reserves no space when disabled", () => {
  fixture.state = {
    statuses: [],
    widgets: [],
    title: null,
    link: null,
    editor: null,
    vibe: {
      enabled: true,
      workers: [
        {
          id: "worker-fast",
          parentId: "agent",
          status: "running",
          description: "Fast worker",
          model: "deepseek-v4.1-flash",
        },
      ],
    },
  };
  const view = render(
    <ProviderRuntimePanel serverId="server" agentId="agent" placement="aboveEditor" />,
  );
  expect(screen.getByTestId("provider-vibe")).toBeTruthy();
  expect(screen.queryByText("worker-fast")).toBeNull();
  expect(
    screen.getByTestId("provider-control-row").contains(screen.getByTestId("provider-vibe")),
  ).toBe(true);
  expect(screen.queryByTestId("provider-vibe-exit")).toBeNull();
  expect(screen.queryByTestId("provider-control-toggle-vibe")).toBeNull();
  expect(screen.queryByTestId("provider-control-content-vibe")).toBeNull();
  view.unmount();

  // An older daemon sends no `vibe` at all: the panel must stay hidden.
  fixture.state = { statuses: [], widgets: [], title: null, link: null, editor: null };
  const legacy = render(
    <ProviderRuntimePanel serverId="server" agentId="agent" placement="aboveEditor" />,
  );
  expect(legacy.container.innerHTML).toBe("");
  legacy.unmount();

  // Disabled vibe with workers is still not a reason to render.
  fixture.state = {
    statuses: [],
    widgets: [],
    title: null,
    link: null,
    editor: null,
    vibe: { enabled: false, workers: [] },
  };
  const disabled = render(
    <ProviderRuntimePanel serverId="server" agentId="agent" placement="aboveEditor" />,
  );
  expect(disabled.container.innerHTML).toBe("");
  fixture.state = { ...fixture.state, vibe: { enabled: true, workers: [] } };
  disabled.rerender(
    <ProviderRuntimePanel serverId="server" agentId="agent" placement="aboveEditor" />,
  );
  expect(screen.getByTestId("provider-vibe")).toBeTruthy();
});

it("does not reserve message-area space for toolbar actions", () => {
  fixture.state = {
    statuses: [],
    widgets: [],
    title: null,
    link: null,
    editor: null,
    actions: [{ id: "get_session_stats", label: "Session statistics", fields: [] }],
  };
  const view = render(
    <ProviderRuntimePanel serverId="server" agentId="agent" placement="aboveEditor" />,
  );
  expect(view.container.innerHTML).toBe("");
});

it("keeps exactly one side conversation as workers become active and finish repeatedly", () => {
  fixture.state = {
    statuses: [],
    widgets: [],
    title: null,
    link: null,
    editor: null,
    sideQuestions: {
      records: [],
      error: "Side conversation loaded",
      omitted: 0,
      reveal: "explicit-btw-open",
      runningId: null,
    },
    vibe: { enabled: true, workers: [] },
  };
  const props = { serverId: "server", agentId: "agent", placement: "aboveEditor" as const };
  const view = render(<ProviderRuntimePanel {...props} />);
  for (const active of [true, false, true, false]) {
    fixture.state = {
      ...fixture.state,
      vibe: {
        enabled: true,
        workers: [
          {
            id: "worker",
            parentId: "agent",
            status: active ? "running" : "completed",
            description: "Worker",
            model: "model",
          },
        ],
      },
    };
    view.rerender(<ProviderRuntimePanel {...props} />);
    expect(screen.getAllByTestId("mock-side-question-panel")).toHaveLength(1);
    expect(Boolean(screen.queryByTestId("provider-vibe"))).toBe(true);
  }
});

it("keeps Vibe informational while the main turn is running and follows native mode changes", () => {
  fixture.mainTurnActive = true;
  fixture.state = {
    statuses: [],
    widgets: [],
    title: null,
    link: null,
    editor: null,
    vibe: {
      enabled: true,
      workers: [
        { id: "worker", parentId: "agent", status: "running", description: "Task", model: null },
      ],
    },
  };
  const view = render(
    <ProviderRuntimePanel serverId="server" agentId="agent" placement="aboveEditor" blocked />,
  );
  fireEvent.click(screen.getByTestId("provider-vibe"));
  expect(fixture.run).not.toHaveBeenCalled();
  expect(screen.queryByRole("button")).toBeNull();
  fixture.state = {
    ...fixture.state,
    vibe: { enabled: false, workers: fixture.state.vibe!.workers },
  };
  view.rerender(
    <ProviderRuntimePanel serverId="server" agentId="agent" placement="aboveEditor" blocked />,
  );
  expect(view.container.innerHTML).toBe("");
});

it("offers draft recovery after a plugin fills the editor and binds it to the owning conversation", () => {
  fixture.state = {
    statuses: [],
    widgets: [],
    title: null,
    link: null,
    editor: { id: "recoverable-plugin-draft", text: "Plugin text" },
  };
  const restore = vi.fn();
  const onEditor = vi.fn(() => restore);
  const view = render(
    <ProviderRuntimePanel
      serverId="server"
      agentId="agent"
      placement="aboveEditor"
      onEditor={onEditor}
    />,
  );
  fireEvent.click(screen.getByTestId("provider-editor-restore"));
  expect(restore).toHaveBeenCalledOnce();
  expect(screen.queryByTestId("provider-editor-restore")).toBeNull();
  view.rerender(
    <ProviderRuntimePanel serverId="other-server" agentId="other-owner" placement="aboveEditor" />,
  );
  expect(screen.queryByTestId("provider-editor-restore")).toBeNull();
});

it("keeps authoritative Goal controls above the editor and binds actions to the displayed goal", async () => {
  fixture.state = {
    statuses: [],
    widgets: [],
    title: null,
    editor: null,
    link: null,
    goal: {
      type: "provider_command",
      id: "current-goal",
      command: "goal",
      status: "success",
      text: "Goal active",
      goal: {
        provider: "omp",
        id: "native-goal",
        phase: "active",
        objective: "Complete the real Tai task",
      },
      actions: [
        { id: "pause", command: "/goal pause" },
        { id: "clear", command: "/goal drop" },
      ],
    },
    vibe: { enabled: true, workers: [] },
  };
  const view = render(
    <ProviderRuntimePanel serverId="server" agentId="agent" placement="aboveEditor" />,
  );
  expect(screen.getByText("Complete the real Tai task")).toBeTruthy();
  const goalToggle = screen.getByTestId("provider-control-toggle-goal:native-goal");
  if (goalToggle.getAttribute("aria-expanded") !== "true") fireEvent.click(goalToggle);
  fireEvent.click(goalToggle);
  expect(screen.getByTestId("provider-control-content-goal:native-goal").style.display).toBe(
    "none",
  );
  expect(fixture.run).not.toHaveBeenCalled();
  expect(screen.getByTestId("provider-vibe")).toBeTruthy();
  expect(screen.queryByTestId("provider-control-toggle-vibe")).toBeNull();
  fireEvent.click(screen.getByTestId("provider-control-toggle-goal:native-goal"));
  await act(async () => fireEvent.click(screen.getByText("providerControl.actions.pause")));
  expect(fixture.run).toHaveBeenCalledExactlyOnceWith("agent", "goal", {
    op: "pause",
    expectedGoalId: "native-goal",
  });
  // RPC admission alone is not a confirmed paused state; only the owner snapshot changes it.
  expect(screen.getAllByText("providerControl.status.active").length).toBeGreaterThan(0);
  fixture.state = { ...fixture.state, goal: undefined };
  view.rerender(<ProviderRuntimePanel serverId="server" agentId="agent" placement="aboveEditor" />);
  expect(screen.queryByTestId("provider-control-toggle-goal:native-goal")).toBeNull();
});

it("refuses a stale Goal button after the owner receives another native goal", async () => {
  fixture.state = {
    statuses: [],
    widgets: [],
    title: null,
    editor: null,
    link: null,
    goal: {
      type: "provider_command",
      id: "current",
      command: "goal",
      status: "success",
      text: "",
      goal: { provider: "omp", id: "old-goal", phase: "active", objective: "Old objective" },
      actions: [{ id: "pause", command: "/goal pause" }],
    },
  };
  render(<ProviderRuntimePanel serverId="server" agentId="agent" placement="aboveEditor" />);
  const toggle = screen.getByTestId("provider-control-toggle-goal:old-goal");
  if (toggle.getAttribute("aria-expanded") !== "true") fireEvent.click(toggle);
  fixture.state = {
    ...fixture.state,
    goal: {
      ...fixture.state.goal!,
      goal: { provider: "omp", id: "new-goal", phase: "active", objective: "New objective" },
    },
  };
  await act(async () => fireEvent.click(screen.getByText("providerControl.actions.pause")));
  expect(fixture.run).not.toHaveBeenCalled();
  expect(screen.getByText("providerControl.goalChanged")).toBeTruthy();
});

it("keeps historical BTW hidden until requested, and ending removes both panel and pill until another request", () => {
  fixture.state = {
    statuses: [],
    widgets: [],
    title: null,
    link: null,
    editor: null,
    sideQuestions: {
      records: [{ id: "old", question: "Old question", createdAt: 1, truncated: false, turns: [] }],
      error: "Old error",
      omitted: 0,
      runningId: null,
      reveal: null,
    },
  };
  const props = { serverId: "server", agentId: "agent", placement: "aboveEditor" as const };
  const mounted = render(<ProviderRuntimePanel {...props} />);
  expect(screen.queryByTestId("provider-control-toggle-side-questions")).toBeNull();
  fixture.state = {
    ...fixture.state,
    sideQuestions: { ...fixture.state.sideQuestions!, reveal: "explicit-one" },
  };
  mounted.rerender(<ProviderRuntimePanel {...props} />);
  expect(
    screen.getByTestId("provider-control-toggle-side-questions").getAttribute("aria-expanded"),
  ).toBe("true");
  fireEvent.click(screen.getByText("End BTW"));
  expect(screen.queryByTestId("provider-control-toggle-side-questions")).toBeNull();
  expect(screen.queryByTestId("mock-side-question-panel")).toBeNull();
  fixture.state = {
    ...fixture.state,
    sideQuestions: { ...fixture.state.sideQuestions!, runningId: "old" },
  };
  mounted.rerender(<ProviderRuntimePanel {...props} />);
  expect(screen.queryByTestId("mock-side-question-panel")).toBeNull();
  mounted.unmount();
  const returned = render(<ProviderRuntimePanel {...props} />);
  expect(screen.queryByTestId("provider-control-toggle-side-questions")).toBeNull();
  fixture.state = {
    ...fixture.state,
    sideQuestions: { ...fixture.state.sideQuestions!, reveal: "explicit-two" },
  };
  returned.rerender(<ProviderRuntimePanel {...props} />);
  expect(
    screen.getByTestId("provider-control-toggle-side-questions").getAttribute("aria-expanded"),
  ).toBe("true");
});

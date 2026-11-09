// @vitest-environment jsdom
import React from "react";
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ProviderUiState } from "@ait/protocol/messages";
import { ProviderRuntimePanel } from "./provider-runtime-panel";

const fixture = vi.hoisted(() => ({
  state: null as ProviderUiState | null,
  open: vi.fn(async () => {}),
}));
vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeClient: () => ({ runNativeAction: vi.fn() }),
}));
vi.mock("@/stores/session-store", () => ({
  useSessionStore: (selector: (state: unknown) => unknown) =>
    selector({
      sessions: { server: { agents: new Map([["agent", { providerUi: fixture.state }]]) } },
    }),
}));
vi.mock("@/utils/open-external-url", () => ({ openExternalUrl: fixture.open }));
vi.mock("./provider-side-questions", () => ({
  ProviderSideQuestions: () => <div data-testid="mock-side-question-panel">Side conversation</div>,
}));
vi.mock("@/hooks/use-settings", () => ({
  useSettings: (selector: (state: unknown) => unknown) =>
    selector({ openInSidePane: { subagents: false } }),
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: () => "Open in browser" }) }));
afterEach(() => {
  cleanup();
  fixture.open.mockClear();
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
  expect(screen.getByText("Build ready")).toBeTruthy();
  expect(screen.queryByText(/First/)).toBeNull();
  expect(fixture.open).not.toHaveBeenCalled();
  fireEvent.click(screen.getByText("Open in browser"));
  expect(fixture.open).toHaveBeenCalledExactlyOnceWith("https://example.com/launch");
  render(<ProviderRuntimePanel serverId="server" agentId="agent" placement="belowEditor" />);
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

it("shows active director controls and reserves no space for idle Vibe", () => {
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
  expect(screen.getByTestId("provider-vibe-exit")).toBeTruthy();
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
  expect(disabled.container.innerHTML).toBe("");
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
      reveal: null,
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
    expect(Boolean(screen.queryByTestId("provider-vibe"))).toBe(active);
  }
});

// @vitest-environment jsdom
import React from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { BotView } from "@ait/protocol/bot/types";
import { WorkspaceBotTab } from "./workspace-bot-tab";

const { workspaceBot, canManage } = vi.hoisted(() => ({
  workspaceBot: vi.fn(),
  canManage: vi.fn(),
}));

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("./styles", () => ({ botStyles: {} }));
vi.mock("./loading", () => ({ BotLoadingSpinner: () => <div data-testid="bot-loading" /> }));
vi.mock("@/hosts/background-task-access", () => ({ canManageBackgroundTasks: canManage }));
vi.mock("./queries", () => ({ useWorkspaceBot: workspaceBot }));
vi.mock("./bot-setup", () => ({ BotSetup: () => <div data-testid="bot-setup" /> }));
vi.mock("./bot-settings", () => ({ BotSettings: () => <div data-testid="bot-settings-editor" /> }));
vi.mock("./bot-chat", () => ({ BotChat: () => <div data-testid="bot-history" /> }));

const activeBot: BotView = {
  workspaceId: "workspace",
  execution: { provider: "codex", model: null, modeId: null, thinking: null },
  builtinChannelId: "22222222-2222-4222-8222-222222222222",
  state: { kind: "active" },
  createdAt: "2026-11-06",
};

function mockWorkspaceBot(result: { data?: BotView | null; isPending?: boolean }) {
  workspaceBot.mockReturnValue({
    data: result.data,
    isPending: result.isPending ?? false,
    error: null,
    refetch: vi.fn(),
  });
}

function mockCanManage(value: boolean) {
  canManage.mockReturnValue(value);
}

function tabElement(view?: "settings") {
  return (
    <WorkspaceBotTab
      serverId="server"
      workspaceId="workspace"
      cwd="/repo"
      view={view}
      interactive
      onChat={vi.fn()}
      onSettings={vi.fn()}
    />
  );
}

function renderTab(view?: "settings") {
  return render(tabElement(view));
}

beforeEach(() => {
  mockCanManage(true);
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

it("shows a spinner while the workspace bot loads", () => {
  mockWorkspaceBot({ isPending: true });
  renderTab();
  expect(screen.getByTestId("bot-loading")).toBeTruthy();
  expect(screen.queryByTestId("bot-setup")).toBeNull();
});

it("shows the setup wizard when the workspace has no bot", () => {
  mockWorkspaceBot({ data: null });
  renderTab();
  expect(screen.getByTestId("bot-setup")).toBeTruthy();
  expect(screen.queryByTestId("bot-show-chat")).toBeNull();
});

it("shows a read-only hint instead of the wizard for guests", () => {
  mockWorkspaceBot({ data: null });
  mockCanManage(false);
  renderTab();
  expect(screen.queryByTestId("bot-setup")).toBeNull();
  expect(screen.getByText("bots.notConfigured")).toBeTruthy();
});

it("wizard_switches_to_chat_when_bot_appears", () => {
  mockWorkspaceBot({ data: null });
  const { rerender } = renderTab();
  mockWorkspaceBot({ data: activeBot });
  rerender(tabElement());
  expect(screen.getByTestId("bot-history")).toBeTruthy();
  expect(screen.queryByTestId("bot-setup")).toBeNull();
});

it("shows settings for the settings view and toggles between chat and settings", () => {
  mockWorkspaceBot({ data: activeBot });
  renderTab("settings");
  expect(screen.getByTestId("bot-settings-editor")).toBeTruthy();
  expect(screen.queryByTestId("bot-history")).toBeNull();
  expect(screen.getByTestId("bot-show-chat")).toBeTruthy();
  expect(screen.getByTestId("bot-show-settings")).toBeTruthy();
});

it("lets guests view settings of a configured bot", () => {
  mockWorkspaceBot({ data: activeBot });
  mockCanManage(false);
  renderTab("settings");
  expect(screen.getByTestId("bot-settings-editor")).toBeTruthy();
});

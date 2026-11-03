// @vitest-environment jsdom
import React from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { WorkspaceBotTab } from "./workspace-bot-tab";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("./styles", () => ({ botStyles: {} }));
vi.mock("./loading", () => ({ BotLoadingSpinner: () => null }));
vi.mock("@/runtime/host-runtime", () => ({ useHostRuntimeClient: () => null }));
vi.mock("@tanstack/react-query", () => ({
  useQuery: () => ({ data: [], isPending: false, error: null }),
}));
vi.mock("./queries", () => ({
  botKeys: { list: () => ["bots"] },
  useBotDetail: (_serverId: string, botId?: string) => ({
    data: botId ? { id: botId, workspaceId: "workspace", name: "Helper" } : undefined,
    isPending: false,
  }),
}));
vi.mock("./bot-form", () => ({ BotForm: () => <div data-testid="bot-editor" /> }));
vi.mock("./bot-settings", () => ({ BotSettings: () => <div data-testid="bot-settings-editor" /> }));
vi.mock("./bot-chat", () => ({ BotChat: () => <div data-testid="bot-history" /> }));

afterEach(cleanup);

function open(serverId: string, view: "create" | "settings") {
  render(
    <WorkspaceBotTab
      serverId={serverId}
      workspaceId="workspace"
      cwd="/repo"
      botId={view === "settings" ? "bot" : undefined}
      view={view}
      interactive
      onBrowse={vi.fn()}
      onCreate={vi.fn()}
      onOpen={vi.fn()}
      onSettings={vi.fn()}
    />,
  );
}

it("restores a shared Bot create tab as a list without a creation action", () => {
  open("shared:grant", "create");
  expect(screen.getByText("bots.empty")).toBeTruthy();
  expect(screen.queryByTestId("bot-editor")).toBeNull();
  expect(screen.queryByTestId("bot-create-new")).toBeNull();
});

it("restores shared Bot settings as conversation history", () => {
  open("shared:grant", "settings");
  expect(screen.getByTestId("bot-history")).toBeTruthy();
  expect(screen.queryByTestId("bot-settings-editor")).toBeNull();
  expect(screen.queryByTestId("bot-show-settings")).toBeNull();
});

it.each(["create", "settings"] as const)("keeps the owner %s editor", (view) => {
  open("owner", view);
  expect(screen.getByTestId(view === "create" ? "bot-editor" : "bot-settings-editor")).toBeTruthy();
});

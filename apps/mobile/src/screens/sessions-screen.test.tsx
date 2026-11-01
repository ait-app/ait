// @vitest-environment jsdom
import React from "react";
import { cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { SessionsScreen } from "./sessions-screen";

const history = vi.hoisted(() => ({
  agents: [] as { id: string; title: string }[],
  hasMore: false,
  isInitialLoad: false,
  isLoadingMore: false,
  isError: true,
  isSearchSupported: false,
  isSearchTruncated: false,
  hostErrors: [{ serverId: "offline", serverName: "Offline Host" }],
  loadMore: vi.fn(),
  refreshAll: vi.fn(async () => {}),
}));
vi.mock("@react-navigation/native", () => ({ useIsFocused: () => true }));
vi.mock("@/hooks/use-agent-history", () => ({ useAgentHistory: () => history }));
vi.mock("@/hooks/use-import-session", () => ({
  useImportSession: () => ({ open: vi.fn(), sheet: null }),
}));
vi.mock("@/runtime/host-runtime", () => ({
  useHosts: () => [{ serverId: "offline", label: "Offline Host" }],
}));
vi.mock("@/components/headers/menu-header", () => ({ MenuHeader: () => null }));
vi.mock("@/components/hosts/host-filter", () => ({ HostFilter: () => null }));
vi.mock("@/components/agent-list", () => ({
  AgentList: ({ agents }: { agents: typeof history.agents }) => (
    <div>
      {agents.map((agent) => (
        <div key={agent.id}>{agent.title}</div>
      ))}
    </div>
  ),
}));

beforeEach(async () => {
  history.agents = [];
  history.isError = true;
  await i18n.changeLanguage("en");
});
afterEach(cleanup);

it("shows the ordinary empty history when hosts are offline or requests fail", () => {
  const view = render(<SessionsScreen />);
  expect(view.getByTestId("sessions-empty")).toBeTruthy();
  expect(view.queryByTestId("sessions-host-errors")).toBeNull();
  expect(view.queryByText("Unable to load sessions")).toBeNull();
});

it("keeps readable history without showing unavailable host banners", () => {
  history.agents = [{ id: "readable", title: "Available history" }];
  history.isError = false;
  const view = render(<SessionsScreen />);
  expect(view.getByText("Available history")).toBeTruthy();
  expect(view.queryByTestId("sessions-host-errors")).toBeNull();
});

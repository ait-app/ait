// @vitest-environment jsdom
import { createElement } from "react";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HubAccountSnapshot, HubShare } from "@ait/client/internal/hub";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { i18n } from "@/i18n/i18next";
import { selectIncomingShares, SharedResourcesScreen } from "./shared-resources-screen";

const state = vi.hoisted(() => ({
  hosts: [
    { serverId: "supported", label: "Current daemon" },
    { serverId: "unsupported", label: "Old daemon" },
  ],
  clients: new Map<string, Pick<DaemonClient, "listShares">>(),
}));

vi.mock("@/runtime/host-runtime", () => ({
  useHosts: () => state.hosts,
  isSharedResourceHost: () => false,
  getHostRuntimeStore: () => ({ getClient: (serverId: string) => state.clients.get(serverId) }),
}));
vi.mock("@/runtime/hub-service-store", () => ({
  useHubServices: (select: (store: { snapshot: null }) => unknown) => select({ snapshot: null }),
}));
vi.mock("@/components/headers/menu-header", () => ({ MenuHeader: () => null }));
vi.mock("@/components/sidebar/shared-resource-list", () => ({ SharedResourceList: () => null }));
vi.mock("lucide-react-native", () => ({
  FolderGit2: () => null,
  Monitor: () => null,
  RefreshCw: () => null,
  Share2: () => null,
  Waypoints: () => null,
  X: () => null,
}));

beforeEach(async () => {
  state.clients.clear();
  for (const host of state.hosts) {
    state.clients.set(host.serverId, {
      listShares: vi.fn<DaemonClient["listShares"]>().mockRejectedValue(
        Object.assign(new Error("Rust daemon does not implement share.list.request"), {
          code: "not_implemented",
        }),
      ),
    });
  }
  await i18n.changeLanguage("en");
});
afterEach(cleanup);

describe("selectIncomingShares", () => {
  it("keeps an empty fallback reference stable for Zustand selectors", () => {
    expect(selectIncomingShares(null)).toBe(selectIncomingShares(null));
    expect(selectIncomingShares({ shares: null })).toBe(selectIncomingShares({}));
  });

  it("returns the Hub share snapshot unchanged", () => {
    const shares = [{ shareId: "share-1" }] as HubShare[];
    const snapshot = { shares } as Pick<HubAccountSnapshot, "shares">;
    expect(selectIncomingShares(snapshot)).toBe(shares);
  });
});

describe("SharedResourcesScreen", () => {
  it("shows outgoing shares when another daemon does not implement sharing", async () => {
    state.clients.set("supported", {
      listShares: vi.fn<DaemonClient["listShares"]>().mockResolvedValue({
        requestId: "list-shares",
        groups: [{ groupId: "group-1", name: "Team", status: "active", myRole: "member" }],
        shares: [
          {
            resourceKind: "project",
            resourceKey: "project-1",
            groupId: "group-1",
            permission: "operate",
            audience: "group",
            recipients: [],
            settingId: "setting-1",
            state: "accepted",
            displayName: "Shared project",
            missingRecipients: [],
          },
        ],
      }),
    });
    const view = render(createElement(SharedResourcesScreen));
    fireEvent.click(view.getByRole("button", { name: "Shared by me (0)" }));

    expect(await view.findByText("Shared project")).toBeTruthy();
    expect(view.getByText("Project · Team · Current daemon")).toBeTruthy();
    expect(view.getByRole("button", { name: "Shared by me (1)" })).toBeTruthy();
    expect(view.queryByText("Rust daemon does not implement share.list.request")).toBeNull();
  });

  it("shows the empty state without an error when no daemon supports sharing", async () => {
    const view = render(createElement(SharedResourcesScreen));
    fireEvent.click(view.getByRole("button", { name: "Shared by me (0)" }));

    expect(await view.findByText("You are not sharing anything")).toBeTruthy();
    expect(view.queryByText("Rust daemon does not implement share.list.request")).toBeNull();
  });

  it.each([
    Object.assign(new Error("Sharing access denied"), { code: "forbidden" }),
    new Error("Connection lost"),
  ])("still reports other request failures: $message", async (error) => {
    state.clients.set("supported", {
      listShares: vi.fn<DaemonClient["listShares"]>().mockRejectedValue(error),
    });
    const view = render(createElement(SharedResourcesScreen));
    fireEvent.click(view.getByRole("button", { name: "Shared by me (0)" }));

    expect(await view.findByText(error.message)).toBeTruthy();
  });
});

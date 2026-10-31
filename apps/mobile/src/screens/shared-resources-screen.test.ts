// @vitest-environment jsdom
import { createElement } from "react";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HubAccountSnapshot, HubShare } from "@ait/client/internal/hub";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { i18n } from "@/i18n/i18next";
import { useHubServices } from "@/runtime/hub-service-store";
import { useSharedHostPreferences } from "@/stores/shared-host-preferences-store";
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
vi.mock("@/runtime/account-state", () => ({
  useAccountState: (select: (store: { center: string; accountId: string }) => unknown) =>
    select({ center: "https://center.test/api", accountId: "recipient" }),
}));
vi.mock("@react-native-async-storage/async-storage", () => ({
  default: { getItem: async () => null, setItem: async () => {}, removeItem: async () => {} },
}));
vi.mock("@/components/headers/menu-header", () => ({ MenuHeader: () => null }));
vi.mock("lucide-react-native", () => ({
  FolderGit2: () => null,
  Monitor: () => null,
  RefreshCw: () => null,
  Share2: () => null,
  Waypoints: () => null,
  X: () => null,
}));

beforeEach(async () => {
  await useSharedHostPreferences.persist.rehydrate();
  useSharedHostPreferences.setState({ disabledHostKeys: [] });
  useHubServices.setState({ snapshot: null });
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
  it("lists each shared Host once with a default-on switch that remains available when turned off", () => {
    const share: HubShare = {
      shareId: "project-grant",
      groupId: "team",
      groupName: "Team",
      hostId: "host",
      nodeId: "node",
      hostName: "Workstation",
      owner: { userId: "owner", displayName: "Owner" },
      resourceKind: "project",
      resourceKey: "project",
      displayName: "Project",
      projectName: "Project",
      permission: "operate",
      state: "online",
    };
    const shares: HubShare[] = [
      share,
      { ...share, shareId: "workspace-grant", resourceKind: "workspace" },
    ];
    useHubServices.setState({
      snapshot: {
        state: "online",
        linkId: "link",
        centerUrl: "https://center.test/api",
        methods: [],
        shares,
      },
    });
    const view = render(createElement(SharedResourcesScreen));
    expect(view.getByRole("button", { name: "Shared with me (1)" })).toBeTruthy();
    expect(view.getAllByRole("switch")).toHaveLength(1);
    const toggle = () => view.getByRole("switch", { name: "Show Owner · Workstation in Hosts" });
    expect(toggle().getAttribute("aria-checked")).toBe("true");
    fireEvent.click(toggle());
    expect(toggle().getAttribute("aria-checked")).toBe("false");
    expect(view.getByText("Owner · Workstation")).toBeTruthy();
    expect(useHubServices.getState().snapshot?.shares).toBe(shares);
    view.unmount();

    const reopened = render(createElement(SharedResourcesScreen));
    const restoredToggle = reopened.getByRole("switch");
    expect(restoredToggle.getAttribute("aria-checked")).toBe("false");
    fireEvent.click(restoredToggle);
    expect(restoredToggle.getAttribute("aria-checked")).toBe("true");
  });

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

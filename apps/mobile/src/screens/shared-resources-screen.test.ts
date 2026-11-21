// @vitest-environment jsdom
import { createElement } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HubShare } from "@ait/client/internal/hub";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { queryClient } from "@/data/query-client";
import { i18n } from "@/i18n/i18next";
import { useHubServices } from "@/runtime/hub-service-store";
import { useSharedHostPreferences } from "@/stores/shared-host-preferences-store";
import { SharedResourcesScreen } from "./shared-resources-screen";

const state = vi.hoisted(() => ({
  hosts: [
    { serverId: "supported", label: "Current daemon" },
    { serverId: "unsupported", label: "Old daemon" },
  ],
  clients: new Map<
    string,
    Pick<DaemonClient, "listShares"> & Partial<Pick<DaemonClient, "removeShare">>
  >(),
  offline: new Set<string>(),
}));

vi.mock("@/runtime/host-runtime", () => ({
  useHosts: () => state.hosts,
  isSharedResourceHost: () => false,
  useHostRuntimeConnectionStatuses: (serverIds: readonly string[]) =>
    new Map(
      serverIds.map((serverId) => [serverId, state.offline.has(serverId) ? "offline" : "online"]),
    ),
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
vi.mock("react-native-reanimated", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-native-reanimated")>();
  const { View } = await import("react-native");
  return {
    ...actual,
    default: { ...actual.default, View },
    useAnimatedStyle: (factory: () => unknown) => factory(),
    useDerivedValue: (factory: () => unknown) => ({ value: factory() }),
    withTiming: (value: unknown) => value,
  };
});

beforeEach(async () => {
  await useSharedHostPreferences.persist.rehydrate();
  useSharedHostPreferences.setState({ disabledHostKeys: [] });
  useHubServices.setState({ snapshot: null });
  queryClient.clear();
  state.clients.clear();
  state.offline.clear();
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

function screen() {
  return createElement(
    QueryClientProvider,
    { client: queryClient },
    createElement(SharedResourcesScreen),
  );
}

describe("SharedResourcesScreen", () => {
  it("lists each shared Host once with a default-on switch that remains available when turned off", async () => {
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
    const view = render(screen());
    expect(view.getByText("Shared with me (1)")).toBeTruthy();
    expect(await view.findByText("You are not sharing anything")).toBeTruthy();
    expect(view.getAllByRole("switch")).toHaveLength(1);
    const toggle = () => view.getByRole("switch", { name: "Show Owner · Workstation in Hosts" });
    expect(toggle().getAttribute("aria-checked")).toBe("true");
    fireEvent.click(toggle());
    expect(toggle().getAttribute("aria-checked")).toBe("false");
    expect(view.getByText("Owner · Workstation")).toBeTruthy();
    expect(useHubServices.getState().snapshot?.shares).toBe(shares);
    view.unmount();

    const reopened = render(screen());
    await reopened.findByText("You are not sharing anything");
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
    const view = render(screen());

    expect(await view.findByText("Shared project")).toBeTruthy();
    expect(view.getByText("Project · Team · Current daemon")).toBeTruthy();
    expect(view.getByText("Shared by me (1)")).toBeTruthy();
    expect(view.getByText("Shared with me (0)")).toBeTruthy();
    expect(view.getByText("Nothing shared with you yet")).toBeTruthy();
    expect(view.queryByRole("button", { name: /^Shared (with|by) me/ })).toBeNull();
    expect(view.queryByText("Rust daemon does not implement share.list.request")).toBeNull();
  });

  it("drops a stopped share and ignores a list request that was already running", async () => {
    const setting = {
      resourceKind: "project" as const,
      resourceKey: "project-1",
      groupId: "group-1",
      permission: "operate" as const,
      audience: "group" as const,
      recipients: [],
      settingId: "setting-1",
      state: "accepted" as const,
      displayName: "Shared project",
      missingRecipients: [],
    };
    const listed = {
      requestId: "list-shares",
      groups: [{ groupId: "group-1", name: "Team", status: "active", myRole: "member" }],
      shares: [setting],
    };
    const slow = Promise.withResolvers<typeof listed>();
    const listShares = vi.fn<DaemonClient["listShares"]>().mockResolvedValueOnce(listed);
    const removeShare = vi.fn<DaemonClient["removeShare"]>().mockResolvedValue({
      requestId: "remove-share",
      resourceKind: "project",
      resourceKey: "project-1",
      groupId: "group-1",
      removed: true,
    });
    state.clients.set("supported", { listShares, removeShare });
    const view = render(screen());
    expect(await view.findByText("Shared project")).toBeTruthy();

    listShares.mockReturnValueOnce(slow.promise);
    fireEvent.click(view.getByRole("button", { name: "Refresh" }));
    fireEvent.click(view.getByRole("button", { name: "Stop sharing" }));

    expect(await view.findByText("You are not sharing anything")).toBeTruthy();
    expect(removeShare).toHaveBeenCalledWith({
      resourceKind: "project",
      resourceKey: "project-1",
      groupId: "group-1",
    });
    await act(async () => slow.resolve(listed));
    expect(view.queryByText("Shared project")).toBeNull();
  });

  it("shows the empty state without an error when no daemon supports sharing", async () => {
    const view = render(screen());

    expect(await view.findByText("You are not sharing anything")).toBeTruthy();
    expect(view.queryByText("Rust daemon does not implement share.list.request")).toBeNull();
  });

  it("keeps other hosts' shares when one daemon fails and skips offline daemons", async () => {
    const listShares = vi.fn<DaemonClient["listShares"]>().mockResolvedValue({
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
    });
    const failing = vi.fn<DaemonClient["listShares"]>().mockRejectedValue(new Error("Timed out"));
    state.clients.set("supported", { listShares });
    state.clients.set("unsupported", { listShares: failing });
    const view = render(screen());

    expect(await view.findByText("Shared project")).toBeTruthy();
    expect(view.getByText("Timed out")).toBeTruthy();
    view.unmount();

    state.offline.add("unsupported");
    failing.mockClear();
    const reopened = render(screen());
    expect(await reopened.findByText("Shared project")).toBeTruthy();
    expect(reopened.queryByText("Timed out")).toBeNull();
    expect(failing).not.toHaveBeenCalled();
  });

  it.each([
    Object.assign(new Error("Sharing access denied"), { code: "forbidden" }),
    new Error("Connection lost"),
  ])("still reports other request failures: $message", async (error) => {
    state.clients.set("supported", {
      listShares: vi.fn<DaemonClient["listShares"]>().mockRejectedValue(error),
    });
    const view = render(screen());

    expect(await view.findByText(error.message)).toBeTruthy();
  });
});

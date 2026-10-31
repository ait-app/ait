// @vitest-environment jsdom
import React from "react";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { HubShare } from "@ait/client/internal/hub";
import { i18n } from "@/i18n/i18next";
import { SharedResourceList } from "./shared-resource-list";

const state = vi.hoisted(() => ({
  hydrated: false,
  connectionStatus: "online",
  workspaceDirectoryError: null as string | null,
  release: vi.fn(),
  refresh: vi.fn(async () => {}),
}));
const share: HubShare = {
  shareId: "share",
  groupId: "group",
  groupName: "Group",
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
vi.mock("@/runtime/hub-service-store", () => ({
  useHubServices: (select: (store: { snapshot: { shares: HubShare[] } }) => unknown) =>
    select({ snapshot: { shares: [share] } }),
}));
vi.mock("@/runtime/host-runtime", () => ({
  sharedResourceServerId: (shareId: string) => `shared:${shareId}`,
  getHostRuntimeStore: () => ({
    acquireDirectoryDemand: () => state.release,
    refreshWorkspaceDirectory: state.refresh,
  }),
  useHostRuntimeSnapshot: () => state,
}));
vi.mock("@/stores/session-store-hooks", () => ({
  useWorkspaceStructure: () => ({ projects: [] }),
  useWorkspace: () => null,
  useHasHydratedWorkspaces: () => state.hydrated,
}));
vi.mock("@/stores/navigation-active-workspace-store", () => ({ navigateToWorkspace: vi.fn() }));
vi.mock("lucide-react-native", () => ({
  ChevronDown: () => null,
  ChevronRight: () => null,
  FolderGit2: () => null,
  Monitor: () => null,
  Users: () => null,
  Waypoints: () => null,
}));

beforeEach(async () => {
  state.hydrated = false;
  state.connectionStatus = "online";
  state.workspaceDirectoryError = null;
  share.state = "online";
  vi.clearAllMocks();
  await i18n.changeLanguage("en");
});
afterEach(cleanup);

it("stops showing loading when the workspace directory fails", () => {
  state.workspaceDirectoryError = "forbidden";
  const view = render(<SharedResourceList />);
  expect(view.queryByText("Loading shared resources…")).toBeNull();
  expect(view.getByText("Could not load shared resources.")).toBeTruthy();
  fireEvent.click(view.getByRole("button", { name: "Retry" }));
  expect(state.refresh).toHaveBeenCalledExactlyOnceWith({ serverId: "shared:share" });
});

it("shows an empty state after an empty directory has loaded", () => {
  state.hydrated = true;
  const view = render(<SharedResourceList />);
  expect(view.queryByText("Loading shared resources…")).toBeNull();
  expect(view.getByText("This shared resource has no workspaces.")).toBeTruthy();
});

it("shows the offline state instead of loading indefinitely", () => {
  state.connectionStatus = "offline";
  const view = render(<SharedResourceList />);
  expect(view.queryByText("Loading shared resources…")).toBeNull();
  expect(view.getByText("This shared resource is offline.")).toBeTruthy();
});

it("shows loading while the first online directory request is pending", () => {
  const view = render(<SharedResourceList />);
  expect(view.getByText("Loading shared resources…")).toBeTruthy();
});

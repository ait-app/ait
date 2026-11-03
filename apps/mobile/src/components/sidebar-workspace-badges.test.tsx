// @vitest-environment jsdom
import { cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HostAppearanceSource, HostBadgeModel } from "@/hosts/appearance";
import { HostBadge } from "@/hosts/host-badge";
import type { SidebarProjectEntry } from "@/hooks/use-sidebar-workspaces-list";
import { SidebarWorkspaceList } from "./sidebar-workspace-list";

const state = vi.hoisted(() => ({
  hosts: [] as HostAppearanceSource[],
  showHost: true,
}));

vi.mock("@/runtime/host-runtime", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/runtime/host-runtime")>()),
  useHosts: () => state.hosts,
}));
vi.mock("@react-native-async-storage/async-storage", () => ({
  default: {
    getItem: async () => null,
    setItem: async () => {},
    removeItem: async () => {},
  },
}));
vi.mock("@/hooks/use-is-local-daemon", () => ({
  useLocalDaemonServerId: () => "local",
  useLocalDaemonServerIdState: () => ({ status: "resolved", serverId: "local" }),
}));
vi.mock("@/runtime/host-features", () => ({ useHostFeatureMap: () => new Map() }));
vi.mock("@/projects/icons", () => ({ useProjectIcons: () => new Map() }));
vi.mock("@/hooks/use-sidebar-workspace-pin", () => ({
  useSidebarWorkspacePinController: () => () => {},
}));
vi.mock("@/components/sidebar/display-preferences/model", () => ({
  useSidebarRowItems: () => ({ host: state.showHost }),
}));
vi.mock("@/components/sidebar/sidebar-status-list", () => ({
  SidebarStatusWorkspaceList: ({
    hostBadgeByServerId,
  }: {
    hostBadgeByServerId: ReadonlyMap<string, HostBadgeModel>;
  }) => (
    <>
      {Array.from(hostBadgeByServerId.values(), (badge) => (
        <HostBadge key={badge.serverId} badge={badge} />
      ))}
    </>
  ),
}));

const serverId = "shared:grant";
const label = "necokeine · shandongdemini.local";
const projects: SidebarProjectEntry[] = [
  {
    viewKey: "project",
    projectName: "Shared project",
    projectKind: "git",
    iconWorkingDir: "/repo",
    hosts: [
      { serverId, projectId: "project", iconWorkingDir: "/repo", worktreeSupport: "supported" },
    ],
    workspaces: [],
  },
];

function Sidebar({ visibleProjects = projects }: { visibleProjects?: SidebarProjectEntry[] }) {
  return (
    <SidebarWorkspaceList
      workspaceGroups={[]}
      projectIconTargets={[]}
      pinnedGroups={{ pinnedChats: [], unpinnedProjects: visibleProjects }}
      projects={visibleProjects}
      hasProjectsBeforeFilter
      hasActiveProjectFilter={false}
      workspaceEntriesByKey={new Map()}
      collapsedProjectKeys={new Set()}
      onToggleProjectCollapsed={() => {}}
      shortcutIndexByWorkspaceKey={new Map()}
      groupMode="status"
    />
  );
}

beforeEach(() => {
  state.hosts = [{ serverId, label, appearance: { color: "none", badgeDisplay: "name" } }];
  state.showHost = true;
});
afterEach(cleanup);

describe("sidebar host badge preference", () => {
  it("shows the configured name when only the shared host has visible projects", () => {
    const view = render(<Sidebar />);
    expect(view.queryByText(label)).not.toBeNull();
  });

  it("shows the same configured name when a second host has visible projects", () => {
    const view = render(
      <Sidebar
        visibleProjects={[
          ...projects,
          {
            ...projects[0],
            viewKey: "local-project",
            hosts: [{ ...projects[0].hosts[0], serverId: "local" }],
          },
        ]}
      />,
    );
    expect(view.queryByText(label)).not.toBeNull();
  });

  it("shows a remote host's default name even with only one visible host", () => {
    state.hosts[0].appearance.badgeDisplay = null;
    const view = render(<Sidebar />);
    expect(view.queryByText(label)).not.toBeNull();
  });

  it("shows only the icon when that is the host's preference", () => {
    state.hosts[0].appearance.badgeDisplay = "icon";
    const view = render(<Sidebar />);
    expect(view.queryByTestId(`host-badge-${serverId}`)).not.toBeNull();
    expect(view.queryByText(label)).toBeNull();
  });

  it("hides a host's badge when that is its preference", () => {
    state.hosts[0].appearance.badgeDisplay = "hidden";
    const view = render(<Sidebar />);
    expect(view.queryByTestId(`host-badge-${serverId}`)).toBeNull();
  });

  it("still respects the sidebar's global host toggle", () => {
    state.showHost = false;
    const view = render(<Sidebar />);
    expect(view.queryByTestId(`host-badge-${serverId}`)).toBeNull();
  });
});

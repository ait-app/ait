// @vitest-environment jsdom
import React from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  SidebarProjectEntry,
  SidebarWorkspaceEntry,
} from "@/hooks/use-sidebar-workspaces-list";
import type { SidebarWorkspaceGroup } from "@/components/sidebar/sidebar-labels";
import { useSidebarViewStore, type SidebarGroupMode } from "@/stores/sidebar-view-store";
import { SidebarWorkspaceList } from "./sidebar-workspace-list";
import { buildSidebarProjection } from "./sidebar/sidebar-projection";

vi.mock("@react-native-async-storage/async-storage", () => ({
  default: { getItem: async () => null, setItem: async () => {}, removeItem: async () => {} },
}));
vi.mock("@/runtime/host-runtime", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/runtime/host-runtime")>()),
  useHosts: () => [],
}));
vi.mock("@/hooks/use-is-local-daemon", () => ({
  useLocalDaemonServerId: () => "host",
  useLocalDaemonServerIdState: () => ({ status: "resolved", serverId: "host" }),
}));
vi.mock("@/runtime/host-features", () => ({
  useHostFeatureMap: () => new Map(),
  useHostFeature: () => false,
}));
vi.mock("@/projects/icons", () => ({ useProjectIcons: () => new Map() }));
vi.mock("@/hooks/use-sidebar-workspace-pin", () => ({
  useSidebarWorkspacePinController: () => () => {},
}));
vi.mock("@/contexts/toast-context", () => ({ useToast: () => ({ error: vi.fn() }) }));
vi.mock("@/hooks/use-keyboard-action-handler", () => ({ useKeyboardActionHandler: () => {} }));
vi.mock("@/components/workspace-hover-card", () => ({
  WorkspaceHoverCard: ({ children }: React.PropsWithChildren) => children,
}));
vi.mock("@/components/sharing/share-manager-sheet", () => ({ ShareManagerSheet: () => null }));

const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

afterEach(() => {
  cleanup();
  queryClient.clear();
  useSidebarViewStore.setState({ hostFilters: [] });
});

function makeWorkspace(projectViewKey: string): SidebarWorkspaceEntry {
  return {
    workspaceKey: `host:${projectViewKey}-0`,
    serverId: "host",
    workspaceId: `${projectViewKey}-0`,
    projectViewKey,
    projectName: projectViewKey,
    projectKind: "git",
    workspaceKind: "worktree",
    name: `${projectViewKey}-0`,
    workspaceDirectory: `/repo/${projectViewKey}/0`,
    workspaceDirectoryLabel: "0",
    title: null,
    currentBranch: null,
    statusBucket: "done",
    statusEnteredAt: new Date(Date.UTC(2026, 0, 1)),
    archivingAt: null,
    diffStat: null,
    prHint: null,
    archiveHasUncommittedChanges: null,
    archiveUnpushedCommitCount: null,
    scripts: [],
    hasRunningScripts: false,
  };
}

interface TestProject extends SidebarProjectEntry {
  workspaces: SidebarWorkspaceEntry[];
}

function makeProject(viewKey: string): TestProject {
  return {
    viewKey,
    projectName: viewKey,
    projectKind: "git",
    iconWorkingDir: `/repo/${viewKey}`,
    hosts: [
      {
        serverId: "host",
        projectId: viewKey,
        iconWorkingDir: `/repo/${viewKey}`,
        worktreeSupport: "supported",
      },
    ],
    workspaces: [makeWorkspace(viewKey)],
  };
}

function Sidebar({
  projects,
  groupMode,
  workspaceGroups = [],
  hasActiveProjectFilter = false,
}: {
  projects: TestProject[];
  groupMode: SidebarGroupMode;
  workspaceGroups?: SidebarWorkspaceGroup[];
  hasActiveProjectFilter?: boolean;
}) {
  const workspaceEntriesByKey = new Map(
    projects.flatMap((project) =>
      project.workspaces.map((workspace) => [workspace.workspaceKey, workspace] as const),
    ),
  );
  const projection = buildSidebarProjection({
    projects,
    workspaceEntriesByKey,
    projectNamesByViewKey: new Map(
      projects.map((project) => [project.viewKey, project.projectName]),
    ),
    pinnedKeys: { pinnedWorkspaceKeys: [], pinnedAtByKey: {} },
    groupMode,
    pinnedCollapsed: false,
    collapsedProjectKeys: new Set(),
    collapsedWorkspaceGroupKeys: new Set(),
  });
  return (
    <QueryClientProvider client={queryClient}>
      <SidebarWorkspaceList
        workspaceGroups={workspaceGroups}
        projectIconTargets={[]}
        pinnedGroups={projection.pinnedGroups}
        projects={projects}
        hasProjectsBeforeFilter
        hasActiveProjectFilter={hasActiveProjectFilter}
        workspaceEntriesByKey={workspaceEntriesByKey}
        collapsedProjectKeys={new Set()}
        onToggleProjectCollapsed={() => {}}
        shortcutIndexByWorkspaceKey={projection.shortcutModel.shortcutIndexByWorkspaceKey}
        groupMode={groupMode}
      />
    </QueryClientProvider>
  );
}

function triggerIds(): string[] {
  return screen
    .queryAllByTestId(/^sidebar-display-preferences-menu-/)
    .map((trigger) => trigger.getAttribute("data-testid")!);
}

describe("where the sidebar's display menu can be opened from", () => {
  it("puts a trigger on every project row and renders no heading above them", () => {
    render(<Sidebar groupMode="project" projects={[makeProject("alpha"), makeProject("beta")]} />);
    expect(triggerIds()).toEqual([
      "sidebar-display-preferences-menu-project-alpha",
      "sidebar-display-preferences-menu-project-beta",
    ]);
    expect(screen.queryByText("Workspaces")).toBeNull();
  });

  it("gives the trigger a row of its own when a filter leaves no project row", () => {
    useSidebarViewStore.setState({ hostFilters: ["elsewhere"] });
    render(<Sidebar groupMode="project" projects={[]} />);
    expect(triggerIds()).toEqual(["sidebar-display-preferences-menu-fallback"]);
    cleanup();

    useSidebarViewStore.setState({ hostFilters: [] });
    render(<Sidebar groupMode="project" projects={[]} hasActiveProjectFilter />);
    expect(triggerIds()).toEqual(["sidebar-display-preferences-menu-fallback"]);
  });

  it("offers no trigger when the sidebar is simply empty", () => {
    render(<Sidebar groupMode="project" projects={[]} />);
    expect(triggerIds()).toEqual([]);
  });

  it("puts a trigger on every status group, and on a row of its own when there is none", () => {
    const project = makeProject("alpha");
    const group: SidebarWorkspaceGroup = {
      key: "done",
      label: "Done",
      rows: project.workspaces,
      leading: { kind: "status", bucket: "done" },
    };
    render(<Sidebar groupMode="status" projects={[project]} workspaceGroups={[group]} />);
    expect(triggerIds()).toEqual(["sidebar-display-preferences-menu-status-done"]);
    cleanup();

    // Without a way back from here the grouping could not be switched to one that has rows.
    render(<Sidebar groupMode="status" projects={[project]} workspaceGroups={[]} />);
    expect(triggerIds()).toEqual(["sidebar-display-preferences-menu-fallback"]);
  });
});

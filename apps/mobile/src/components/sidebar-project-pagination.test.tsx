// @vitest-environment jsdom
import React, { useState } from "react";
import { act, cleanup, fireEvent, render, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { View, type GestureResponderEvent } from "react-native";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PressHighlightProps } from "@/components/ui/press-highlight.types";
import type {
  SidebarProjectEntry,
  SidebarWorkspaceEntry,
} from "@/hooks/use-sidebar-workspaces-list";
import { SidebarWorkspaceList } from "./sidebar-workspace-list";
import { buildSidebarProjection } from "./sidebar/sidebar-projection";

const rowPresses = vi.hoisted(() => new Map<string, PressHighlightProps["onPress"]>());

vi.mock("@/components/ui/press-highlight", () => ({
  PressHighlight: (props: PressHighlightProps) => {
    if (props.testID) rowPresses.set(props.testID, props.onPress);
    return <View testID={props.testID} />;
  },
}));
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

afterEach(() => {
  cleanup();
  rowPresses.clear();
});

interface TestProject extends SidebarProjectEntry {
  workspaces: SidebarWorkspaceEntry[];
}

function makeProject(viewKey: string, count: number): TestProject {
  const workspaces: SidebarWorkspaceEntry[] = Array.from({ length: count }, (_, index) => ({
    workspaceKey: `host:${viewKey}-${index}`,
    serverId: "host",
    workspaceId: `${viewKey}-${index}`,
    projectViewKey: viewKey,
    projectName: viewKey,
    projectKind: "git",
    workspaceKind: "worktree",
    name: `${viewKey}-${index}`,
    workspaceDirectory: `/repo/${viewKey}/${index}`,
    workspaceDirectoryLabel: `${index}`,
    title: null,
    currentBranch: null,
    statusBucket: "done",
    statusEnteredAt: new Date(Date.UTC(2026, 0, 1, 0, index)),
    archivingAt: null,
    diffStat: null,
    prHint: null,
    archiveHasUncommittedChanges: null,
    archiveUnpushedCommitCount: null,
    scripts: [],
    hasRunningScripts: false,
  }));
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
    workspaces,
  };
}

function Sidebar({ projects }: { projects: TestProject[] }) {
  const [collapsedProjectKeys, setCollapsedProjectKeys] = useState(new Set<string>());
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
    groupMode: "project",
    pinnedCollapsed: false,
    collapsedProjectKeys,
    collapsedWorkspaceGroupKeys: new Set(),
  });
  return (
    <SidebarWorkspaceList
      workspaceGroups={[]}
      projectIconTargets={[]}
      pinnedGroups={projection.pinnedGroups}
      projects={projects}
      hasProjectsBeforeFilter
      hasActiveProjectFilter={false}
      workspaceEntriesByKey={workspaceEntriesByKey}
      collapsedProjectKeys={collapsedProjectKeys}
      onToggleProjectCollapsed={(key) =>
        setCollapsedProjectKeys((current) => {
          const next = new Set(current);
          if (next.has(key)) next.delete(key);
          else next.add(key);
          return next;
        })
      }
      shortcutIndexByWorkspaceKey={projection.shortcutModel.shortcutIndexByWorkspaceKey}
      groupMode="project"
    />
  );
}

function renderSidebar(projects: TestProject[]) {
  const client = new QueryClient({ defaultOptions: { queries: { enabled: false, retry: false } } });
  return render(<Sidebar projects={projects} />, {
    wrapper: ({ children }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    ),
  });
}

function workspaceRows(container: HTMLElement) {
  return within(container).queryAllByTestId(/^sidebar-workspace-row-/);
}

function toggleProject(viewKey: string) {
  const press = rowPresses.get(`sidebar-project-row-${viewKey}`);
  if (!press) throw new Error(`Missing project header: ${viewKey}`);
  act(() => press({} as GestureResponderEvent));
}

describe("project workspace pagination", () => {
  it.each([0, 1, 5])("does not offer more workspaces when the project has %i", (count) => {
    const view = renderSidebar([makeProject("project", count)]);
    expect(workspaceRows(view.container)).toHaveLength(count);
    expect(view.queryByTestId("sidebar-project-show-more-project")).toBeNull();
  });

  it("starts with the five most recent workspaces and reveals ten more on each press", () => {
    const view = renderSidebar([makeProject("project", 28), makeProject("other", 7)]);
    const list = view.getByTestId("sidebar-workspace-list-project");
    const more = () => view.getByTestId("sidebar-project-show-more-project");
    expect(workspaceRows(list).map((row) => row.dataset.testid)).toEqual(
      [27, 26, 25, 24, 23].map((index) => `sidebar-workspace-row-host:project-${index}`),
    );

    fireEvent.click(more());
    expect(workspaceRows(list)).toHaveLength(15);
    fireEvent.click(more());
    expect(workspaceRows(list)).toHaveLength(25);
    fireEvent.click(more());
    expect(workspaceRows(list).map((row) => row.dataset.testid)).toEqual(
      Array.from({ length: 28 }, (_, index) => `sidebar-workspace-row-host:project-${27 - index}`),
    );
    expect(view.queryByTestId("sidebar-project-show-more-project")).toBeNull();
    expect(workspaceRows(view.getByTestId("sidebar-workspace-list-other"))).toHaveLength(5);
  });

  it("resets a collapsed project to five workspaces without resetting other projects", () => {
    const view = renderSidebar([makeProject("project", 16), makeProject("other", 16)]);
    fireEvent.click(view.getByTestId("sidebar-project-show-more-project"));
    fireEvent.click(view.getByTestId("sidebar-project-show-more-other"));
    expect(workspaceRows(view.getByTestId("sidebar-workspace-list-project"))).toHaveLength(15);

    toggleProject("project");
    expect(view.queryByTestId("sidebar-workspace-list-project")).toBeNull();
    expect(view.queryByTestId("sidebar-project-show-more-project")).toBeNull();
    toggleProject("project");
    expect(workspaceRows(view.getByTestId("sidebar-workspace-list-project"))).toHaveLength(5);
    expect(workspaceRows(view.getByTestId("sidebar-workspace-list-other"))).toHaveLength(15);

    fireEvent.click(view.getByTestId("sidebar-project-show-more-project"));
    fireEvent.click(view.getByTestId("sidebar-project-show-more-project"));
    expect(view.queryByTestId("sidebar-project-show-more-project")).toBeNull();
    toggleProject("project");
    toggleProject("project");
    expect(workspaceRows(view.getByTestId("sidebar-workspace-list-project"))).toHaveLength(5);
    expect(view.getByTestId("sidebar-project-show-more-project")).toBeTruthy();
  });
});

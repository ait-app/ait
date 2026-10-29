import { describe, expect, it } from "vitest";
import type {
  SidebarProjectEntry,
  SidebarWorkspaceEntry,
  SidebarWorkspacePlacement,
} from "@/hooks/use-sidebar-workspaces-list";
import { buildSidebarProjection } from "./sidebar-projection";

function makeWorkspace(
  id: string,
  statusBucket: SidebarWorkspaceEntry["statusBucket"] = "done",
  labels: string[] = [],
  projectViewKey = "project",
) {
  const placement: SidebarWorkspacePlacement = {
    workspaceKey: `srv:${id}`,
    serverId: "srv",
    workspaceId: id,
    projectViewKey,
    projectName: "Project",
    projectKind: "git",
    workspaceKind: "worktree",
    name: id,
  };
  const entry: SidebarWorkspaceEntry = {
    ...placement,
    workspaceDirectory: "",
    workspaceDirectoryLabel: "",
    title: null,
    currentBranch: null,
    statusBucket,
    statusEnteredAt: null,
    archivingAt: null,
    diffStat: null,
    prHint: null,
    archiveHasUncommittedChanges: null,
    archiveUnpushedCommitCount: null,
    scripts: [],
    hasRunningScripts: false,
    labels,
  };
  return { placement, entry };
}

function makeProject(
  workspaces: SidebarWorkspacePlacement[],
  viewKey = "project",
): SidebarProjectEntry {
  return {
    viewKey,
    projectName: "Project",
    projectKind: "git",
    iconWorkingDir: `/repo/${viewKey}`,
    hosts: [
      {
        serverId: "srv",
        projectId: viewKey,
        iconWorkingDir: `/repo/${viewKey}`,
        worktreeSupport: "supported" as const,
      },
    ],
    workspaces,
  };
}

function projectionInput(options?: {
  groupMode?: "project" | "status";
  pinnedCollapsed?: boolean;
}) {
  const pinned = makeWorkspace("pinned", "running");
  const unpinned = makeWorkspace("unpinned", "needs_input");
  return {
    projects: [makeProject([pinned.placement, unpinned.placement])],
    pinnedKeys: {
      pinnedWorkspaceKeys: [pinned.placement.workspaceKey],
      pinnedAtByKey: { [pinned.placement.workspaceKey]: "2026-07-12T12:00:00.000Z" },
    },
    workspaceEntriesByKey: new Map([
      [pinned.entry.workspaceKey, pinned.entry],
      [unpinned.entry.workspaceKey, unpinned.entry],
    ]),
    projectNamesByViewKey: new Map([["project", "Project"]]),
    groupMode: options?.groupMode ?? ("project" as const),
    pinnedCollapsed: options?.pinnedCollapsed ?? false,
    collapsedProjectKeys: new Set<string>(),
    collapsedWorkspaceGroupKeys: new Set<string>(),
  };
}

/**
 * Two projects, one workspace each, both labelled — so every grouping mode puts rows from more
 * than one project on screen, and a mode that asked for fewer icons than it renders would show it.
 */
function twoProjectInput(groupMode: "project" | "status") {
  const first = makeWorkspace("first", "running", ["Urgent"], "project");
  const second = makeWorkspace("second", "needs_input", ["Backend"], "other-project");
  return {
    ...projectionInput({ groupMode }),
    projects: [makeProject([first.placement]), makeProject([second.placement], "other-project")],
    pinnedKeys: { pinnedWorkspaceKeys: [], pinnedAtByKey: {} },
    workspaceEntriesByKey: new Map([
      [first.entry.workspaceKey, first.entry],
      [second.entry.workspaceKey, second.entry],
    ]),
    projectNamesByViewKey: new Map([
      ["project", "Project"],
      ["other-project", "Other project"],
    ]),
  };
}

describe("buildSidebarProjection", () => {
  it("orders project rows and shortcuts by status entry time across different statuses", () => {
    const older = makeWorkspace("a-older", "needs_input");
    const newer = makeWorkspace("z-newer", "done");
    const pinned = makeWorkspace("pinned", "running");
    older.entry.statusEnteredAt = new Date("2026-01-01T00:00:00Z");
    newer.entry.statusEnteredAt = new Date("2026-02-01T00:00:00Z");
    pinned.entry.statusEnteredAt = new Date("2026-03-01T00:00:00Z");
    const project = makeProject([older.placement, pinned.placement, newer.placement]);
    const input = {
      ...projectionInput(),
      projects: [project],
      workspaceEntriesByKey: new Map(
        [older, pinned, newer].map(({ entry }) => [entry.workspaceKey, entry]),
      ),
    };

    const projection = buildSidebarProjection(input);

    expect(projection.pinnedGroups.unpinnedProjects[0]?.workspaces).toEqual([
      newer.placement,
      older.placement,
    ]);
    expect(projection.shortcutModel.shortcutTargets).toEqual([
      { serverId: "srv", workspaceId: "pinned" },
      { serverId: "srv", workspaceId: "z-newer" },
      { serverId: "srv", workspaceId: "a-older" },
    ]);
    expect(project.workspaces).toEqual([older.placement, pinned.placement, newer.placement]);

    const updated = buildSidebarProjection({
      ...input,
      workspaceEntriesByKey: new Map(input.workspaceEntriesByKey).set(older.entry.workspaceKey, {
        ...older.entry,
        statusEnteredAt: new Date("2026-04-01T00:00:00Z"),
      }),
    });
    expect(updated.pinnedGroups.unpinnedProjects[0]?.workspaces).toEqual([
      older.placement,
      newer.placement,
    ]);
    expect(updated.shortcutModel.shortcutTargets).toEqual([
      { serverId: "srv", workspaceId: "pinned" },
      { serverId: "srv", workspaceId: "a-older" },
      { serverId: "srv", workspaceId: "z-newer" },
    ]);
  });

  it("uses the same time and name tie-breakers in project and status groups", () => {
    const datedZ = makeWorkspace("dated-z");
    const datedA = makeWorkspace("dated-a");
    const undatedZ = makeWorkspace("undated-z");
    const undatedA = makeWorkspace("undated-a");
    const duplicate = makeWorkspace("duplicate");
    duplicate.entry.name = datedA.entry.name;
    datedZ.entry.statusEnteredAt = new Date("2026-01-01T00:00:00Z");
    datedA.entry.statusEnteredAt = datedZ.entry.statusEnteredAt;
    duplicate.entry.statusEnteredAt = datedZ.entry.statusEnteredAt;
    const workspaces = [undatedZ, datedZ, duplicate, undatedA, datedA];
    const input = {
      ...projectionInput(),
      projects: [makeProject(workspaces.map(({ placement }) => placement))],
      pinnedKeys: { pinnedWorkspaceKeys: [], pinnedAtByKey: {} },
      workspaceEntriesByKey: new Map(workspaces.map(({ entry }) => [entry.workspaceKey, entry])),
    };

    const project = buildSidebarProjection(input);
    const status = buildSidebarProjection({ ...input, groupMode: "status" });
    const expected = ["dated-a", "duplicate", "dated-z", "undated-a", "undated-z"];

    expect(
      project.pinnedGroups.unpinnedProjects[0]?.workspaces.map((ws) => ws.workspaceId),
    ).toEqual(expected);
    expect(status.workspaceGroups[0]?.rows.map((ws) => ws.workspaceId)).toEqual(expected);
    expect(project.shortcutModel.shortcutTargets).toEqual(status.shortcutModel.shortcutTargets);
  });

  it("keeps project shells and falls back to names while workspace entries hydrate", () => {
    const z = makeWorkspace("z");
    const a = makeWorkspace("a");
    const empty = makeProject([], "empty");
    const input = {
      ...projectionInput(),
      projects: [makeProject([z.placement, a.placement]), empty],
      pinnedKeys: { pinnedWorkspaceKeys: [], pinnedAtByKey: {} },
      workspaceEntriesByKey: new Map<string, SidebarWorkspaceEntry>(),
    };

    const projection = buildSidebarProjection(input);

    expect(projection.pinnedGroups.unpinnedProjects[0]?.workspaces).toEqual([
      a.placement,
      z.placement,
    ]);
    expect(projection.pinnedGroups.unpinnedProjects[1]).toBe(empty);
  });

  // The rule that outlived the bug it was written for: a project icon is fetched per project, so
  // whatever a mode groups by, the rows it produces can only reference projects already covered.
  for (const groupMode of ["project", "status"] as const) {
    it(`covers every row ${groupMode} grouping renders with a project icon target`, () => {
      const projection = buildSidebarProjection(twoProjectInput(groupMode));
      const covered = new Set(projection.projectIconTargets.map((target) => target.projectViewKey));

      // Every leading visual the sidebar can paint from this projection: pinned rows, grouped
      // rows, project headers and the rows under them.
      const renderedProjectViewKeys = new Set<string>();
      for (const entry of projection.pinnedGroups.pinnedChats) {
        renderedProjectViewKeys.add(entry.projectViewKey);
      }
      for (const group of projection.workspaceGroups) {
        for (const entry of group.rows) renderedProjectViewKeys.add(entry.projectViewKey);
      }
      for (const project of projection.pinnedGroups.unpinnedProjects) {
        renderedProjectViewKeys.add(project.viewKey);
        for (const entry of project.workspaces) renderedProjectViewKeys.add(entry.projectViewKey);
      }

      expect([...renderedProjectViewKeys].sort()).toEqual(["other-project", "project"]);
      expect([...renderedProjectViewKeys].filter((viewKey) => !covered.has(viewKey))).toEqual([]);
    });
  }

  it("uses one pin-aware projection for project rows and shortcut order", () => {
    const projection = buildSidebarProjection(projectionInput());

    expect(projection.pinnedGroups.pinnedChats.map((entry) => entry.workspaceId)).toEqual([
      "pinned",
    ]);
    const remainingProject = projection.pinnedGroups.unpinnedProjects[0];
    expect(remainingProject?.workspaces.map((entry) => entry.workspaceId)).toEqual(["unpinned"]);
    expect(projection.shortcutModel.shortcutTargets).toEqual([
      { serverId: "srv", workspaceId: "pinned" },
      { serverId: "srv", workspaceId: "unpinned" },
    ]);
  });

  it("keeps pinned chats above status groups and removes them from those groups", () => {
    const projection = buildSidebarProjection(projectionInput({ groupMode: "status" }));

    expect(projection.workspaceGroups.map((group) => group.key)).toEqual(["needs_input"]);
    expect(projection.workspaceGroups[0]?.rows.map((entry) => entry.workspaceId)).toEqual([
      "unpinned",
    ]);
    expect(projection.shortcutModel.shortcutTargets).toEqual([
      { serverId: "srv", workspaceId: "pinned" },
      { serverId: "srv", workspaceId: "unpinned" },
    ]);
  });

  it("does not number pinned chats while the pinned section is collapsed", () => {
    const projection = buildSidebarProjection(
      projectionInput({ groupMode: "status", pinnedCollapsed: true }),
    );

    expect(projection.shortcutModel.shortcutTargets).toEqual([
      { serverId: "srv", workspaceId: "unpinned" },
    ]);
  });
});

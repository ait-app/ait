import { buildStatusGroups } from "@/hooks/sidebar-status-view-model";
import {
  splitPinnedSidebarGroups,
  type PinnedSidebarGroups,
  type PinnedSidebarKeys,
} from "@/hooks/use-sidebar-pins";
import type {
  SidebarProjectEntry,
  SidebarWorkspaceEntry,
} from "@/hooks/use-sidebar-workspaces-list";
import type { SidebarGroupMode } from "@/stores/sidebar-view-store";
import {
  resolveSidebarProjectIconTargets,
  type SidebarProjectIconTarget,
} from "@/utils/sidebar-project-row-model";
import {
  buildSidebarShortcutSections,
  type SidebarShortcutModel,
  type SidebarShortcutSection,
} from "@/utils/sidebar-shortcuts";
import { statusWorkspaceGroups, type SidebarWorkspaceGroup } from "./sidebar-labels";
import { compareSidebarWorkspaces } from "@/utils/sidebar-workspace-order";

export interface SidebarProjection {
  pinnedGroups: PinnedSidebarGroups;
  workspaceGroups: SidebarWorkspaceGroup[];
  /**
   * The project icons this projection needs fetched, keyed by `projectViewKey` — one per project,
   * whatever the mode groups by. It sits here rather than beside `useProjectIcons` in the list
   * because it is the same `projects` the rows above are projected from: a mode that renders a
   * row can only ever ask for an icon this list already covers. It used to be derived in the
   * list, under a `groupMode === "status"` gate written when status was the only mode that put
   * icons on rows.
   */
  projectIconTargets: SidebarProjectIconTarget[];
  shortcutModel: SidebarShortcutModel;
}

export interface SidebarProjectionInput {
  projects: SidebarProjectEntry[];
  pinnedKeys: PinnedSidebarKeys;
  pinnedProjectKeys?: readonly string[];
  workspaceEntriesByKey: ReadonlyMap<string, SidebarWorkspaceEntry>;
  projectNamesByViewKey: Map<string, string>;
  groupMode: SidebarGroupMode;
  pinnedCollapsed: boolean;
  collapsedProjectKeys: ReadonlySet<string>;
  collapsedWorkspaceGroupKeys: ReadonlySet<string>;
}

export function buildSidebarProjection(input: SidebarProjectionInput): SidebarProjection {
  const pinnedGroups = splitPinnedSidebarGroups({
    // Include pinned chats when computing project recency.
    projects: input.groupMode === "project" ? orderProjectsByActivity(input) : input.projects,
    keys: input.pinnedKeys,
    keepInProjects: input.groupMode === "project",
  });
  if (input.groupMode === "project") {
    pinnedGroups.unpinnedProjects = pinnedGroups.unpinnedProjects.map((project) => {
      const workspaces = [...project.workspaces].sort(
        (a, b) =>
          Number(input.pinnedKeys.pinnedWorkspaceKeys.includes(b.workspaceKey)) -
            Number(input.pinnedKeys.pinnedWorkspaceKeys.includes(a.workspaceKey)) ||
          (input.pinnedKeys.pinnedAtByKey[b.workspaceKey] ?? "").localeCompare(
            input.pinnedKeys.pinnedAtByKey[a.workspaceKey] ?? "",
          ) ||
          compareSidebarWorkspaces(
            input.workspaceEntriesByKey.get(a.workspaceKey) ?? a,
            input.workspaceEntriesByKey.get(b.workspaceKey) ?? b,
            input.projectNamesByViewKey,
          ),
      );
      return workspaces.every((workspace, index) => workspace === project.workspaces[index])
        ? project
        : { ...project, workspaces };
    });
  }
  const projectPinRank = new Map((input.pinnedProjectKeys ?? []).map((key, index) => [key, index]));
  pinnedGroups.unpinnedProjects = [...pinnedGroups.unpinnedProjects].sort(
    (a, b) =>
      (projectPinRank.get(a.viewKey) ?? projectPinRank.size) -
      (projectPinRank.get(b.viewKey) ?? projectPinRank.size),
  );
  const pinnedWorkspaceKeys = new Set(input.pinnedKeys.pinnedWorkspaceKeys);
  const unpinnedWorkspaces = Array.from(input.workspaceEntriesByKey.values()).filter(
    (workspace) => !pinnedWorkspaceKeys.has(workspace.workspaceKey),
  );
  // One switch decides both what the list groups by and what the keyboard shortcuts walk, so the
  // two cannot disagree and a new grouping mode is a compile error here rather than a silent
  // fall-through to the project rows.
  const workspaceGroups = buildWorkspaceGroups(input, unpinnedWorkspaces);

  const sections: SidebarShortcutSection[] = [];
  if (!input.pinnedCollapsed) {
    sections.push({ workspaces: pinnedGroups.pinnedChats });
  }
  if (input.groupMode === "project") {
    sections.push(
      ...pinnedGroups.unpinnedProjects.map((project) => ({
        workspaces: project.workspaces,
        collapsed: input.collapsedProjectKeys.has(project.viewKey),
      })),
    );
  } else {
    sections.push(
      ...workspaceGroups.map((group) => ({
        workspaces: group.rows,
        collapsed: input.collapsedWorkspaceGroupKeys.has(group.key),
      })),
    );
  }

  return {
    pinnedGroups,
    workspaceGroups,
    projectIconTargets: resolveSidebarProjectIconTargets(input.projects),
    shortcutModel: buildSidebarShortcutSections({ sections }),
  };
}

function orderProjectsByActivity(input: SidebarProjectionInput): SidebarProjectEntry[] {
  return (
    input.projects
      .map((project) => {
        let lastActivityAt = Number.NEGATIVE_INFINITY;
        for (const workspace of project.workspaces) {
          const timestamp = input.workspaceEntriesByKey
            .get(workspace.workspaceKey)
            ?.statusEnteredAt?.getTime();
          if (timestamp !== undefined && Number.isFinite(timestamp)) {
            lastActivityAt = Math.max(lastActivityAt, timestamp);
          }
        }
        return { project, lastActivityAt };
      })
      // Stable ties retain the structure's name order, including projects without activity.
      .sort((a, b) =>
        a.lastActivityAt === b.lastActivityAt ? 0 : b.lastActivityAt - a.lastActivityAt,
      )
      .map(({ project }) => project)
  );
}

/** Project mode keeps its project headers and groups nothing; status mode groups the rows. */
function buildWorkspaceGroups(
  input: SidebarProjectionInput,
  unpinnedWorkspaces: SidebarWorkspaceEntry[],
): SidebarWorkspaceGroup[] {
  switch (input.groupMode) {
    case "project":
      return [];
    case "status":
      return statusWorkspaceGroups(
        buildStatusGroups(unpinnedWorkspaces, input.projectNamesByViewKey),
      );
  }
}

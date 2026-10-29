import type { SidebarWorkspacePlacement } from "@/hooks/sidebar-workspaces-view-model";

type SidebarSortableWorkspace = Pick<
  SidebarWorkspacePlacement,
  "workspaceKey" | "projectViewKey" | "name"
> & { statusEnteredAt?: Date | null };

export function compareSidebarWorkspaces(
  a: SidebarSortableWorkspace,
  b: SidebarSortableWorkspace,
  projectNamesByViewKey: ReadonlyMap<string, string>,
): number {
  const aTime = a.statusEnteredAt?.getTime() ?? null;
  const bTime = b.statusEnteredAt?.getTime() ?? null;

  if (aTime !== null && bTime !== null) {
    if (aTime !== bTime) return bTime - aTime;
  } else if (aTime !== null) {
    return -1;
  } else if (bTime !== null) {
    return 1;
  }

  const aProject = projectNamesByViewKey.get(a.projectViewKey) ?? "";
  const bProject = projectNamesByViewKey.get(b.projectViewKey) ?? "";
  const projectCmp = aProject.localeCompare(bProject);
  if (projectCmp !== 0) return projectCmp;

  const nameCmp = a.name.localeCompare(b.name);
  if (nameCmp !== 0) return nameCmp;

  return a.workspaceKey.localeCompare(b.workspaceKey);
}

import type { SidebarWorkspacePlacement } from "@/hooks/use-sidebar-workspaces-list";

export interface SidebarShortcutWorkspaceTarget {
  serverId: string;
  workspaceId: string;
}

/** Number of workspaces reachable through the Cmd/Ctrl/Alt+digit shortcuts. */
export const SIDEBAR_SHORTCUT_LIMIT = 9;

export interface SidebarShortcutModel {
  /** Visible workspaces with a digit shortcut, capped at the shortcut limit. */
  shortcutTargets: SidebarShortcutWorkspaceTarget[];
  /** Every visible workspace in visual order, for next/previous workspace navigation. */
  navigationTargets: SidebarShortcutWorkspaceTarget[];
  shortcutIndexByWorkspaceKey: Map<string, number>;
}

export interface SidebarShortcutSection {
  workspaces: readonly SidebarWorkspacePlacement[];
  collapsed?: boolean;
}

function createShortcutTarget(
  workspace: SidebarWorkspacePlacement,
): SidebarShortcutWorkspaceTarget {
  return {
    serverId: workspace.serverId,
    workspaceId: workspace.workspaceId,
  };
}

export function buildSidebarShortcutSections(input: {
  sections: readonly SidebarShortcutSection[];
}): SidebarShortcutModel {
  const shortcutTargets: SidebarShortcutWorkspaceTarget[] = [];
  const navigationTargets: SidebarShortcutWorkspaceTarget[] = [];
  const shortcutIndexByWorkspaceKey = new Map<string, number>();

  for (const section of input.sections) {
    if (section.collapsed) {
      continue;
    }

    for (const workspace of section.workspaces) {
      const target = createShortcutTarget(workspace);
      navigationTargets.push(target);
      if (shortcutTargets.length >= SIDEBAR_SHORTCUT_LIMIT) {
        continue;
      }

      const shortcutNumber = shortcutTargets.length + 1;
      shortcutTargets.push(target);
      shortcutIndexByWorkspaceKey.set(workspace.workspaceKey, shortcutNumber);
    }
  }

  return { shortcutTargets, navigationTargets, shortcutIndexByWorkspaceKey };
}

export function getRelativeSidebarShortcutTarget(input: {
  targets: readonly SidebarShortcutWorkspaceTarget[];
  currentTarget: SidebarShortcutWorkspaceTarget | null;
  delta: 1 | -1;
}): SidebarShortcutWorkspaceTarget | null {
  if (input.targets.length === 0) {
    return null;
  }

  if (!input.currentTarget) {
    return input.targets[input.delta > 0 ? 0 : input.targets.length - 1] ?? null;
  }

  const currentTarget = input.currentTarget;
  const currentIndex = input.targets.findIndex(
    (target) =>
      target.serverId === currentTarget.serverId &&
      target.workspaceId === currentTarget.workspaceId,
  );
  if (currentIndex < 0) {
    return input.targets[input.delta > 0 ? 0 : input.targets.length - 1] ?? null;
  }

  const nextIndex = (currentIndex + input.delta + input.targets.length) % input.targets.length;
  return input.targets[nextIndex] ?? null;
}

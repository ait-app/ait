import { buildWorkspaceTabPersistenceKey } from "@/workspace-tabs/model";
import type { ActiveWorkspaceSelection } from "@/stores/navigation-active-workspace-store";
import type { WorkspaceDescriptor } from "@/stores/session-store";
import { trimNonEmpty } from "@/utils/string";

export interface CompactExplorerSidebarHostModel {
  serverId: string;
  workspaceId: string;
  persistenceKey: string;
  workspaceRoot: string;
  isGit: boolean;
}

interface ResolveCompactExplorerSidebarHostModelInput {
  previous: CompactExplorerSidebarHostModel | null;
  selection: ActiveWorkspaceSelection | null;
  workspace: WorkspaceDescriptor | null;
  isGit: boolean;
}

export function resolveCompactExplorerSidebarHostModel(
  input: ResolveCompactExplorerSidebarHostModelInput,
): CompactExplorerSidebarHostModel | null {
  const serverId = trimNonEmpty(input.selection?.serverId);
  const workspaceId = trimNonEmpty(input.selection?.workspaceId);
  if (!serverId || !workspaceId) {
    return null;
  }

  const persistenceKey = buildWorkspaceTabPersistenceKey({ serverId, workspaceId });
  if (!persistenceKey) {
    return null;
  }

  const previousForSelection =
    input.previous &&
    input.previous.serverId === serverId &&
    input.previous.workspaceId === workspaceId
      ? input.previous
      : null;

  return {
    serverId,
    workspaceId,
    persistenceKey,
    workspaceRoot:
      trimNonEmpty(input.workspace?.workspaceDirectory) ??
      previousForSelection?.workspaceRoot ??
      "",
    isGit: input.workspace ? input.isGit : (previousForSelection?.isGit ?? input.isGit),
  };
}

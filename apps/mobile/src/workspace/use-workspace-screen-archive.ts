import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useCheckoutStatusQuery } from "@/git/use-status-query";
import { type WorktreeArchiveWarningLabels } from "@/git/worktree-archive-warning";
import { useActiveWorkspaceSelection } from "@/stores/navigation-active-workspace-store";
import { useWorkspaceFields } from "@/stores/session-store-hooks";
import { redirectIfArchivingActiveWorkspace } from "@/workspace/sidebar-workspace-archive-redirect";
import { useWorkspaceArchive } from "@/workspace/use-workspace-archive";

interface WorkspaceScreenArchive {
  archive: () => void;
  /** False until the workspace is known and, for a worktree, its archive risk can be judged. */
  canArchive: boolean;
  isArchiving: boolean;
}

/**
 * Archive for the workspace that is open on screen, as opposed to a sidebar row. It prefers the
 * live checkout status over the directory's cached git summary when judging what an archive would
 * discard, and leaves the workspace once the archive starts if it is the one being viewed.
 */
export function useWorkspaceScreenArchive({
  serverId,
  workspaceId,
}: {
  serverId: string;
  workspaceId: string;
}): WorkspaceScreenArchive {
  const { t } = useTranslation();
  const activeWorkspaceSelection = useActiveWorkspaceSelection();
  const [isHidingWorkspace, setIsHidingWorkspace] = useState(false);
  const workspace = useWorkspaceFields(serverId, workspaceId, (descriptor) => ({
    id: descriptor.id,
    name: descriptor.name,
    workspaceKind: descriptor.workspaceKind,
    workspaceDirectory: descriptor.workspaceDirectory ?? null,
    isDirty: descriptor.gitRuntime?.isDirty,
    aheadOfOrigin: descriptor.gitRuntime?.aheadOfOrigin,
    diffStat: descriptor.diffStat ?? null,
    isArchiving: descriptor.archivingAt != null,
  }));
  const { status } = useCheckoutStatusQuery({
    serverId,
    cwd: workspace?.workspaceDirectory ?? "",
  });
  const gitStatus = status?.isGit ? status : null;
  const isDirty = gitStatus?.isDirty ?? workspace?.isDirty;
  const aheadOfOrigin = gitStatus?.aheadOfOrigin ?? workspace?.aheadOfOrigin;
  const warningLabels = useMemo(() => getWorktreeArchiveWarningLabels(t), [t]);

  const handleArchiveStarted = useCallback(() => {
    redirectIfArchivingActiveWorkspace({ serverId, workspaceId, activeWorkspaceSelection });
  }, [activeWorkspaceSelection, serverId, workspaceId]);

  const { archive } = useWorkspaceArchive({
    serverId,
    workspaceId: workspace?.id ?? "",
    workspaceKind: workspace?.workspaceKind ?? "directory",
    name: workspace?.name ?? "",
    isDirty,
    aheadOfOrigin,
    diffStat: workspace?.diffStat ?? null,
    warningLabels,
    onSetHiding: setIsHidingWorkspace,
    onArchiveStarted: handleArchiveStarted,
  });

  return {
    archive,
    canArchive:
      workspace !== null &&
      (workspace.workspaceKind !== "worktree" ||
        (isDirty !== undefined && aheadOfOrigin !== undefined)),
    isArchiving: Boolean(workspace?.isArchiving) || isHidingWorkspace,
  };
}

function getWorktreeArchiveWarningLabels(
  t: (key: string, options?: Record<string, unknown>) => string,
): WorktreeArchiveWarningLabels {
  return {
    title: (workspaceName) => t("workspace.git.actions.archiveWarning.title", { workspaceName }),
    confirm: t("workspace.git.actions.archiveWarning.confirm"),
    cancel: t("workspace.git.actions.archiveWarning.cancel"),
    uncommittedChanges: t("workspace.git.actions.archiveWarning.uncommittedChanges"),
    uncommittedChangesWithDiff: (diffStat) =>
      t("workspace.git.actions.archiveWarning.uncommittedChangesWithDiff", { diffStat }),
    addedLine: (count) =>
      t(
        count === 1
          ? "workspace.git.actions.archiveWarning.addedLine"
          : "workspace.git.actions.archiveWarning.addedLines",
        { count },
      ),
    deletedLine: (count) =>
      t(
        count === 1
          ? "workspace.git.actions.archiveWarning.deletedLine"
          : "workspace.git.actions.archiveWarning.deletedLines",
        { count },
      ),
    unpushedCommit: (count) =>
      t(
        count === 1
          ? "workspace.git.actions.archiveWarning.unpushedCommit"
          : "workspace.git.actions.archiveWarning.unpushedCommits",
        { count },
      ),
  };
}

import { useCallback, useState, type ReactElement } from "react";
import { useTranslation } from "react-i18next";
import {
  ShareManagerSheet,
  type ShareResourceTarget,
} from "@/components/sharing/share-manager-sheet";
import { WorkspaceRenameModal } from "@/components/workspace-rename-modal";
import { useToast } from "@/contexts/toast-context";
import { toWorktreeArchiveRisk } from "@/git/worktree-archive-warning";
import { useKeyboardActionHandler } from "@/hooks/use-keyboard-action-handler";
import { useShortcutKeys } from "@/hooks/use-shortcut-keys";
import type { SidebarWorkspaceEntry } from "@/hooks/use-sidebar-workspaces-list";
import type { ToggleSidebarWorkspacePin } from "@/hooks/use-sidebar-workspace-pin";
import { useWorkspaceClipboardActions } from "@/hooks/use-workspace-clipboard-actions";
import { useWorkspaceReadState } from "@/hooks/use-workspace-read-state";
import type { SidebarSurfaceBackdrop } from "@/styles/surface-backdrop";
import type { ShortcutKey } from "@/utils/format-shortcut";
import { redirectIfArchivingActiveWorkspace } from "@/workspace/sidebar-workspace-archive-redirect";
import { useWorkspaceArchive } from "@/workspace/use-workspace-archive";
import { SidebarWorkspaceMenu } from "@/components/sidebar/sidebar-workspace-menu";
import {
  SidebarWorkspaceTrailingActionBase,
  SidebarWorkspaceTrailingActionOverlay,
  SidebarWorkspaceTrailingActionSlot,
  type SidebarWorkspaceTrailingPresentation,
} from "@/components/sidebar/sidebar-workspace-row-content";
import { useOpenKebabMenuVisibility } from "@/components/sidebar/use-open-kebab-menu-visibility";
import {
  SidebarWorkspaceTrailingContent,
  type SidebarWorkspaceTrailing,
} from "@/components/sidebar/workspace-trailing";

/** The menu actions every sidebar workspace row offers, in both its context menu and its kebab. */
export interface SidebarWorkspaceRowActions {
  onArchive: () => void;
  archiveStatus: "idle" | "pending";
  archiveShortcutKeys: ShortcutKey[][] | null;
  onCopyPath: () => void;
  onCopyBranchName?: () => void;
  onRename: () => void;
  onMarkAsRead?: () => void;
  onMarkAsUnread?: () => void;
  isPinned: boolean;
  onTogglePin?: () => void;
  onShare: () => void;
  openInFileManagerPath: string;
}

/**
 * Archive, clipboard, rename, read-state, pin and share handling for one sidebar workspace row,
 * shared by the project-mode and grouped-mode rows. The row renders `renameModal` and
 * `shareSheet` beside itself.
 */
export function useSidebarWorkspaceRowActions({
  workspace,
  selected,
  canCopyBranchName,
  canPin,
  onToggleWorkspacePin,
}: {
  workspace: SidebarWorkspaceEntry;
  selected: boolean;
  canCopyBranchName: boolean;
  canPin: boolean;
  onToggleWorkspacePin: ToggleSidebarWorkspacePin;
}): {
  isArchiving: boolean;
  actions: SidebarWorkspaceRowActions;
  renameModal: ReactElement;
  shareSheet: ReactElement;
} {
  const { t } = useTranslation();
  const toast = useToast();
  const [isHidingWorkspace, setIsHidingWorkspace] = useState(false);
  const [isRenameOpen, setIsRenameOpen] = useState(false);
  const isArchiving = workspace.archivingAt !== null || isHidingWorkspace;

  const redirectAfterArchive = useCallback(() => {
    redirectIfArchivingActiveWorkspace({
      serverId: workspace.serverId,
      workspaceId: workspace.workspaceId,
      activeWorkspaceSelection: selected
        ? { serverId: workspace.serverId, workspaceId: workspace.workspaceId }
        : null,
    });
  }, [selected, workspace]);

  const archiveController = useWorkspaceArchive({
    serverId: workspace.serverId,
    workspaceId: workspace.workspaceId,
    workspaceKind: workspace.workspaceKind,
    name: workspace.name,
    ...toWorktreeArchiveRisk(workspace),
    onArchiveStarted: redirectAfterArchive,
    onSetHiding: setIsHidingWorkspace,
  });

  const handleArchive = useCallback(() => {
    if (isArchiving) return;
    archiveController.archive();
  }, [archiveController, isArchiving]);

  const clipboard = useWorkspaceClipboardActions();
  const handleCopyPath = useCallback(() => {
    clipboard.copyPath(workspace);
  }, [clipboard, workspace]);
  const handleCopyBranchName = useCallback(() => {
    clipboard.copyBranchName(workspace);
  }, [clipboard, workspace]);

  const handleOpenRename = useCallback(() => setIsRenameOpen(true), []);
  const handleCloseRename = useCallback(() => setIsRenameOpen(false), []);
  const handleTogglePin = useCallback(() => {
    onToggleWorkspacePin(workspace);
  }, [onToggleWorkspacePin, workspace]);

  const archiveShortcutKeys = useShortcutKeys("archive-workspace");
  const { hasClearableAttention, canMarkUnread, clearAttention, markUnread } =
    useWorkspaceReadState({
      serverId: workspace.serverId,
      workspaceId: workspace.workspaceId,
    });
  const handleMarkAsRead = useCallback(() => {
    void clearAttention().catch((error) => {
      toast.error(
        error instanceof Error ? error.message : t("sidebar.workspace.toasts.markAsReadFailed"),
      );
    });
  }, [clearAttention, t, toast]);
  const handleMarkAsUnread = useCallback(() => {
    void markUnread().catch((error) => {
      toast.error(
        error instanceof Error ? error.message : t("sidebar.workspace.toasts.markAsUnreadFailed"),
      );
    });
  }, [markUnread, t, toast]);

  const [shareTarget, setShareTarget] = useState<ShareResourceTarget | null>(null);
  const handleShare = useCallback(
    () =>
      setShareTarget({
        serverId: workspace.serverId,
        resourceKind: "workspace",
        resourceKey: workspace.workspaceId,
        displayName: workspace.name,
        projectName: workspace.projectName,
      }),
    [workspace],
  );
  const closeShare = useCallback(() => setShareTarget(null), []);

  useKeyboardActionHandler({
    handlerId: `workspace-archive-${workspace.workspaceKey}`,
    actions: ["workspace.archive"],
    enabled: selected && !isArchiving,
    priority: 0,
    handle: () => {
      handleArchive();
      return true;
    },
  });

  return {
    isArchiving,
    actions: {
      onArchive: handleArchive,
      archiveStatus: isArchiving ? "pending" : "idle",
      archiveShortcutKeys: selected ? archiveShortcutKeys : null,
      onCopyPath: handleCopyPath,
      onCopyBranchName: canCopyBranchName ? handleCopyBranchName : undefined,
      onRename: handleOpenRename,
      onMarkAsRead: hasClearableAttention ? handleMarkAsRead : undefined,
      onMarkAsUnread: canMarkUnread ? handleMarkAsUnread : undefined,
      isPinned: workspace.pinnedAt != null,
      onTogglePin: canPin ? handleTogglePin : undefined,
      onShare: handleShare,
      openInFileManagerPath: workspace.workspaceDirectory,
    },
    renameModal: (
      <WorkspaceRenameModal
        visible={isRenameOpen}
        workspace={workspace}
        onClose={handleCloseRename}
        testID={`sidebar-workspace-rename-modal-${workspace.workspaceKey}`}
      />
    ),
    shareSheet: (
      <ShareManagerSheet visible={shareTarget !== null} target={shareTarget} onClose={closeShare} />
    ),
  };
}

/** The trailing slot of a sidebar workspace row: its trailing content, with the kebab over it. */
export function SidebarWorkspaceActionSlot({
  workspace,
  backdrop,
  trailing,
  trailingPresentation,
  showKebab,
  showScrim,
  reserveSlotWidth,
  actions,
}: {
  workspace: SidebarWorkspaceEntry;
  backdrop: SidebarSurfaceBackdrop;
  trailing: SidebarWorkspaceTrailing;
  trailingPresentation: SidebarWorkspaceTrailingPresentation;
  showKebab: boolean;
  showScrim: boolean;
  reserveSlotWidth: boolean;
  actions: SidebarWorkspaceRowActions;
}) {
  const kebab = useOpenKebabMenuVisibility(showKebab);
  return (
    <SidebarWorkspaceTrailingActionSlot reserveWidth={reserveSlotWidth}>
      <SidebarWorkspaceTrailingActionBase presentation={trailingPresentation}>
        <SidebarWorkspaceTrailingContent workspace={workspace} trailing={trailing} />
      </SidebarWorkspaceTrailingActionBase>
      <SidebarWorkspaceTrailingActionOverlay
        visible={kebab.showKebab}
        scrimBackdrop={showScrim ? backdrop : undefined}
      >
        <SidebarWorkspaceMenu
          {...kebab.menuProps}
          {...actions}
          workspaceKey={workspace.workspaceKey}
          serverId={workspace.serverId}
          workspaceId={workspace.workspaceId}
          workspaceLabels={workspace.labels}
        />
      </SidebarWorkspaceTrailingActionOverlay>
    </SidebarWorkspaceTrailingActionSlot>
  );
}

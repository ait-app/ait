import { memo, useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { View, Text } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import type { HostBadgeModel } from "@/hosts/appearance";
import type { SidebarWorkspaceEntry } from "@/hooks/use-sidebar-workspaces-list";
import type { SidebarSurfaceBackdrop } from "@/styles/surface-backdrop";
import type { ShortcutKey } from "@/utils/format-shortcut";
import { WorkspaceRenameModal } from "@/components/workspace-rename-modal";
import { useWorkspaceClipboardActions } from "@/hooks/use-workspace-clipboard-actions";
import { useToast } from "@/contexts/toast-context";
import { toWorktreeArchiveRisk } from "@/git/worktree-archive-warning";
import { useWorkspaceArchive } from "@/workspace/use-workspace-archive";
import { useShortcutKeys } from "@/hooks/use-shortcut-keys";
import { useKeyboardActionHandler } from "@/hooks/use-keyboard-action-handler";
import { useWorkspaceReadState } from "@/hooks/use-workspace-read-state";
import { redirectIfArchivingActiveWorkspace } from "@/utils/sidebar-workspace-archive-redirect";
import { isNative as platformIsNative } from "@/constants/platform";
import { useIsCompactFormFactor } from "@/constants/layout";
import {
  SidebarWorkspaceContextMenu,
  SidebarWorkspaceMenu,
} from "@/components/sidebar/sidebar-workspace-menu";
import {
  SidebarWorkspaceRowFrame,
  SidebarWorkspaceRowContent,
  resolveTrailingActionVisibility,
  SidebarWorkspaceTrailingActionBase,
  SidebarWorkspaceTrailingActionOverlay,
  SidebarWorkspaceTrailingActionSlot,
} from "@/components/sidebar/sidebar-workspace-row-content";
import { useOpenKebabMenuVisibility } from "@/components/sidebar/use-open-kebab-menu-visibility";
import { getSidebarRowBackdrop } from "@/components/sidebar/sidebar-row-backdrop";
import { selectWorkspaceServiceSummary } from "@/components/sidebar/workspace-meta-row";
import {
  SidebarWorkspaceTrailingContent,
  useSidebarWorkspaceTrailing,
  type SidebarWorkspaceTrailing,
} from "@/components/sidebar/workspace-trailing";

interface SidebarWorkspaceRowProps {
  workspace: SidebarWorkspaceEntry;
  selected: boolean;
  shortcutNumber: number | null;
  showShortcutBadge: boolean;
  canCopyBranchName: boolean;
  onPress: () => void;
  /** The host pill after the title. Absent → the sidebar spans one host, or this one is hidden. */
  hostBadge?: HostBadgeModel | null;
  /** Project grouping only: shows a transient "creating" affordance. */
  isCreating?: boolean;
}

export function SidebarWorkspaceRow({
  workspace,
  selected,
  shortcutNumber,
  showShortcutBadge,
  canCopyBranchName,
  onPress,
  hostBadge,
  isCreating = false,
}: SidebarWorkspaceRowProps) {
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
    if (isArchiving) {
      return;
    }
    archiveController.archive();
  }, [archiveController, isArchiving]);

  const clipboard = useWorkspaceClipboardActions();
  const handleCopyPath = useCallback(() => {
    clipboard.copyPath(workspace);
  }, [clipboard, workspace]);

  const handleCopyBranchName = useCallback(() => {
    clipboard.copyBranchName(workspace);
  }, [clipboard, workspace]);

  const handleOpenRename = useCallback(() => {
    setIsRenameOpen(true);
  }, []);

  const handleCloseRename = useCallback(() => {
    setIsRenameOpen(false);
  }, []);

  const archiveShortcutKeys = useShortcutKeys("archive-workspace");
  const { hasClearableAttention, canMarkUnread, clearAttention, markUnread } =
    useWorkspaceReadState({
      serverId: workspace.serverId,
      workspaceId: workspace.workspaceId,
    });
  const handleMarkAsRead = useCallback(() => {
    void clearAttention().catch((error) => {
      toast.error(error instanceof Error ? error.message : "Failed to mark workspace as read");
    });
  }, [clearAttention, toast]);
  const handleMarkAsUnread = useCallback(() => {
    void markUnread().catch((error) => {
      toast.error(error instanceof Error ? error.message : "Failed to mark workspace as unread");
    });
  }, [markUnread, toast]);

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

  return (
    <>
      <WorkspaceRowBody
        workspace={workspace}
        selected={selected}
        shortcutNumber={shortcutNumber}
        showShortcutBadge={showShortcutBadge}
        hostBadge={hostBadge}
        isCreating={isCreating}
        isArchiving={isArchiving}
        onPress={onPress}
        archiveLabel={t("sidebar.workspace.actions.archive")}
        archiveStatus={isArchiving ? "pending" : "idle"}
        archivePendingLabel={t("sidebar.workspace.actions.archiving")}
        onArchive={handleArchive}
        onCopyBranchName={canCopyBranchName ? handleCopyBranchName : undefined}
        onCopyPath={handleCopyPath}
        onRename={handleOpenRename}
        onMarkAsRead={hasClearableAttention ? handleMarkAsRead : undefined}
        onMarkAsUnread={canMarkUnread ? handleMarkAsUnread : undefined}
        archiveShortcutKeys={selected ? archiveShortcutKeys : null}
      />
      <WorkspaceRenameModal
        visible={isRenameOpen}
        workspace={workspace}
        onClose={handleCloseRename}
        testID={`sidebar-workspace-rename-modal-${workspace.workspaceKey}`}
      />
    </>
  );
}

interface WorkspaceRowBodyProps {
  workspace: SidebarWorkspaceEntry;
  selected: boolean;
  shortcutNumber: number | null;
  showShortcutBadge: boolean;
  hostBadge?: HostBadgeModel | null;
  isCreating: boolean;
  isArchiving: boolean;
  onPress: () => void;
  archiveLabel?: string;
  archiveStatus?: "idle" | "pending" | "success";
  archivePendingLabel?: string;
  onArchive?: () => void;
  onCopyBranchName?: () => void;
  onCopyPath?: () => void;
  onRename?: () => void;
  onMarkAsRead?: () => void;
  onMarkAsUnread?: () => void;
  archiveShortcutKeys?: ShortcutKey[][] | null;
}

function WorkspaceRowBody({
  workspace,
  selected,
  shortcutNumber,
  showShortcutBadge,
  hostBadge,
  isCreating,
  isArchiving,
  onPress,
  archiveLabel,
  archiveStatus = "idle",
  archivePendingLabel,
  onArchive,
  onCopyBranchName,
  onCopyPath,
  onRename,
  onMarkAsRead,
  onMarkAsUnread,
  archiveShortcutKeys,
}: WorkspaceRowBodyProps) {
  const isCompact = useIsCompactFormFactor();
  const showSelection = selected && !isCompact;
  const isTouchPlatform = platformIsNative || isCompact;
  const [isPressed, setIsPressed] = useState(false);
  const trailing = useSidebarWorkspaceTrailing();
  const handlePress = onPress;
  const handleWorkspacePressIn = useCallback(() => setIsPressed(true), []);
  const handleWorkspacePressOut = useCallback(() => setIsPressed(false), []);

  const accessibilityState = useMemo(() => ({ selected }), [selected]);

  return (
    <SidebarWorkspaceRowFrame workspace={workspace}>
      {({ isHovered, contextMenuOpen, onContextMenuOpenChange, hoverHandlers }) => {
        const isDesktop = !isTouchPlatform;
        const serviceSummary = isDesktop ? selectWorkspaceServiceSummary(workspace.scripts) : null;
        const workspaceRowStyle = getWorkspaceRowStyle({
          isPressed,
          selected: showSelection,
          isHovered,
        });
        const backdrop = getSidebarRowBackdrop({
          isPressed,
          selected: showSelection,
          isHovered,
        });
        return (
          <View style={styles.workspaceRowContainer} {...hoverHandlers}>
            <SidebarWorkspaceContextMenu
              contextMenuOpen={contextMenuOpen}
              onContextMenuOpenChange={onContextMenuOpenChange}
              workspace={workspace}
              hostBadgeLabel={hostBadge?.label}
              serviceSummary={serviceSummary}
              workspaceKey={workspace.workspaceKey}
              onCopyPath={onCopyPath}
              onCopyBranchName={onCopyBranchName}
              onRename={onRename}
              onMarkAsRead={onMarkAsRead}
              onMarkAsUnread={onMarkAsUnread}
              onArchive={onArchive}
              archiveLabel={archiveLabel}
              archiveStatus={archiveStatus}
              archivePendingLabel={archivePendingLabel}
              archiveShortcutKeys={archiveShortcutKeys}
              openInFileManagerPath={workspace.workspaceDirectory}
              disabled={isArchiving}
              aria-selected={selected}
              accessibilityRole="button"
              accessibilityState={accessibilityState}
              style={workspaceRowStyle}
              highlightStyle={styles.workspaceRowPressed}
              onPressIn={handleWorkspacePressIn}
              onPressOut={handleWorkspacePressOut}
              onPress={handlePress}
              testID={`sidebar-workspace-row-${workspace.workspaceKey}`}
            >
              <SidebarWorkspaceRowContent
                workspace={workspace}
                hostBadge={hostBadge}
                serviceSummary={serviceSummary}
                backdrop={backdrop}
                isHovered={isHovered}
                isLoading={isArchiving || isCreating}
                isCreating={isCreating}
                shortcutNumber={shortcutNumber}
                showShortcutBadge={showShortcutBadge}
              >
                <WorkspaceRowTrailingActions
                  workspace={workspace}
                  backdrop={backdrop}
                  trailing={trailing}
                  isHovered={isHovered}
                  isTouchPlatform={isTouchPlatform}
                  isCreating={isCreating}
                  showShortcutBadge={showShortcutBadge}
                  shortcutNumber={shortcutNumber}
                  archiveLabel={archiveLabel}
                  archiveStatus={archiveStatus}
                  archivePendingLabel={archivePendingLabel}
                  archiveShortcutKeys={archiveShortcutKeys}
                  onArchive={onArchive}
                  onCopyBranchName={onCopyBranchName}
                  onCopyPath={onCopyPath}
                  onRename={onRename}
                  onMarkAsRead={onMarkAsRead}
                  onMarkAsUnread={onMarkAsUnread}
                />
              </SidebarWorkspaceRowContent>
            </SidebarWorkspaceContextMenu>
          </View>
        );
      }}
    </SidebarWorkspaceRowFrame>
  );
}

function WorkspaceRowTrailingActions({
  workspace,
  backdrop,
  trailing,
  isHovered,
  isTouchPlatform,
  isCreating,
  showShortcutBadge,
  shortcutNumber,
  archiveLabel,
  archiveStatus,
  archivePendingLabel,
  archiveShortcutKeys,
  onArchive,
  onMarkAsRead,
  onMarkAsUnread,
  onCopyBranchName,
  onCopyPath,
  onRename,
}: {
  workspace: SidebarWorkspaceEntry;
  backdrop: SidebarSurfaceBackdrop;
  trailing: SidebarWorkspaceTrailing;
  isHovered: boolean;
  isTouchPlatform: boolean;
  isCreating: boolean;
  showShortcutBadge: boolean;
  shortcutNumber: number | null;
  archiveLabel?: string;
  archiveStatus?: "idle" | "pending" | "success";
  archivePendingLabel?: string;
  archiveShortcutKeys?: ShortcutKey[][] | null;
  onArchive?: () => void;
  onMarkAsRead?: () => void;
  onMarkAsUnread?: () => void;
  onCopyBranchName?: () => void;
  onCopyPath?: () => void;
  onRename?: () => void;
}) {
  const { t } = useTranslation();
  const showShortcut = showShortcutBadge && shortcutNumber !== null;
  const {
    trailingPresentation,
    showKebab: showKebabInSlot,
    showScrim,
    renderSlot,
    reserveSlotWidth,
  } = resolveTrailingActionVisibility({
    workspace,
    trailing,
    hasArchiveAction: Boolean(onArchive),
    isHovered,
    isTouchPlatform,
    showShortcut,
  });
  const kebab = useOpenKebabMenuVisibility(showKebabInSlot);

  return (
    <>
      {isCreating ? (
        <Text style={styles.workspaceCreatingText}>{t("sidebar.workspace.status.creating")}</Text>
      ) : null}
      {renderSlot ? (
        <SidebarWorkspaceTrailingActionSlot reserveWidth={reserveSlotWidth}>
          <SidebarWorkspaceTrailingActionBase presentation={trailingPresentation}>
            <SidebarWorkspaceTrailingContent workspace={workspace} trailing={trailing} />
          </SidebarWorkspaceTrailingActionBase>
          <SidebarWorkspaceTrailingActionOverlay
            visible={kebab.showKebab}
            scrimBackdrop={showScrim ? backdrop : undefined}
          >
            {onArchive ? (
              <SidebarWorkspaceMenu
                {...kebab.menuProps}
                workspaceKey={workspace.workspaceKey}
                serverId={workspace.serverId}
                workspaceId={workspace.workspaceId}
                workspaceLabels={workspace.labels}
                onCopyPath={onCopyPath}
                onCopyBranchName={onCopyBranchName}
                onRename={onRename}
                onMarkAsRead={onMarkAsRead}
                onMarkAsUnread={onMarkAsUnread}
                onArchive={onArchive}
                archiveLabel={archiveLabel}
                archiveStatus={archiveStatus}
                archivePendingLabel={archivePendingLabel}
                archiveShortcutKeys={archiveShortcutKeys}
              />
            ) : null}
          </SidebarWorkspaceTrailingActionOverlay>
        </SidebarWorkspaceTrailingActionSlot>
      ) : null}
    </>
  );
}

function getWorkspaceRowStyle({
  isPressed,
  selected,
  isHovered,
}: {
  isPressed: boolean;
  selected: boolean;
  isHovered: boolean;
}) {
  return [
    styles.workspaceRow,
    isHovered && styles.workspaceRowHovered,
    selected && styles.sidebarRowSelected,
    isPressed && styles.workspaceRowPressed,
  ];
}

export const MemoSidebarWorkspaceRow = memo(SidebarWorkspaceRow);

const styles = StyleSheet.create((theme) => ({
  workspaceRowContainer: {
    position: "relative",
  },
  workspaceRow: {
    minHeight: 36,
    marginBottom: theme.spacing[1],
    paddingVertical: theme.spacing[2],
    paddingLeft: theme.spacing[2],
    paddingRight: theme.spacing[3],
    borderRadius: theme.borderRadius.lg,
    flexDirection: "column",
    alignItems: "stretch",
    justifyContent: "center",
    gap: theme.spacing[1],
    userSelect: "none",
  },
  workspaceRowHovered: {
    backgroundColor: theme.colors.surfaceSidebarHover,
  },
  workspaceRowPressed: {
    backgroundColor: theme.colors.surface2,
  },
  sidebarRowSelected: {
    backgroundColor: theme.colors.surfaceSidebarSelected,
  },
  workspaceCreatingText: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    flexShrink: 0,
  },
}));

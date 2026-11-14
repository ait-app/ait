import {
  memo,
  useCallback,
  useMemo,
  type MutableRefObject,
  type ReactElement,
  type ReactNode,
} from "react";
import {
  View,
  Text,
  Pressable,
  ScrollView,
  type PressableStateCallbackType,
  type RefreshControlProps,
} from "react-native";
import { ScrollView as GestureScrollView, type GestureType } from "react-native-gesture-handler";
import { useTranslation } from "react-i18next";
import {
  navigateToWorkspace,
  useActiveWorkspaceSelection,
} from "@/stores/navigation-active-workspace-store";
import { type SidebarWorkspaceEntry } from "@/hooks/use-sidebar-workspaces-list";
import type { StatusBucket } from "@/hooks/sidebar-status-view-model";
import type { SidebarWorkspaceGroup } from "@/components/sidebar/sidebar-labels";
import { SidebarFilterEmptyState } from "@/components/sidebar/empty-states";
import type { HostBadgeModel } from "@/hosts/appearance";
import { isWeb as platformIsWeb, isNative as platformIsNative } from "@/constants/platform";
import { useIsCompactFormFactor } from "@/constants/layout";
import { StyleSheet, withUnistyles } from "react-native-unistyles";
import type { Theme } from "@/styles/theme";
import {
  ChevronDown,
  ChevronRight,
  CircleAlert,
  CircleCheck,
  CircleDot,
  CircleX,
} from "lucide-react-native";
import {
  SidebarWorkspaceRowFrame,
  SidebarWorkspaceRowContent,
  resolveTrailingActionVisibility,
  sidebarWorkspaceRowStyles,
} from "@/components/sidebar/sidebar-workspace-row-content";
import {
  SidebarWorkspaceActionSlot,
  useSidebarWorkspaceRowActions,
} from "@/components/sidebar/sidebar-workspace-row-actions";
import { getSidebarRowBackdrop } from "@/components/sidebar/sidebar-row-backdrop";
import { getStatusDotColor } from "@/utils/status-dot-color";
import { selectWorkspaceServiceSummary } from "@/components/sidebar/workspace-meta-row";
import { useSidebarWorkspaceTrailing } from "@/components/sidebar/workspace-trailing";
import { useSidebarCollapsedSectionsStore } from "@/stores/sidebar-collapsed-sections-store";
import { SidebarWorkspaceContextMenu } from "@/components/sidebar/sidebar-workspace-menu";
import { PinnedSectionHeader } from "@/components/sidebar/pinned-section-header";
import { SidebarGroupToggleRow } from "@/components/sidebar/sidebar-group-toggle-row";
import { useLimitedSidebarGroup } from "@/components/sidebar/use-limited-sidebar-group";
import type { ToggleSidebarWorkspacePin } from "@/hooks/use-sidebar-workspace-pin";
import { useSidebarRowHover } from "@/components/sidebar/use-sidebar-row-hover";
import { useSidebarRowPress } from "@/components/sidebar/use-sidebar-row-press";

// Themed icon wrappers
const foregroundMutedColorMapping = (theme: Theme) => ({
  color: theme.colors.foregroundMuted,
});
// One mapping per bucket, resolved through the status-dot producer so a group header and
// the rows under it cannot disagree about what "failed" looks like.
const needsInputColorMapping = (theme: Theme) => ({
  color: getStatusDotColor({ theme, bucket: "needs_input" }) ?? undefined,
});
const failedColorMapping = (theme: Theme) => ({
  color: getStatusDotColor({ theme, bucket: "failed" }) ?? undefined,
});
const attentionColorMapping = (theme: Theme) => ({
  color: getStatusDotColor({ theme, bucket: "attention" }) ?? undefined,
});
const runningColorMapping = (theme: Theme) => ({
  color: getStatusDotColor({ theme, bucket: "running" }) ?? undefined,
});

const ThemedChevronDown = withUnistyles(ChevronDown);
const ThemedChevronRight = withUnistyles(ChevronRight);
const ThemedCircleAlert = withUnistyles(CircleAlert);
const ThemedCircleCheck = withUnistyles(CircleCheck);
const ThemedCircleDot = withUnistyles(CircleDot);
const ThemedCircleX = withUnistyles(CircleX);
const EMPTY_SHORTCUT_INDEX = new Map<string, number>();

interface StatusWorkspaceListProps {
  groups: SidebarWorkspaceGroup[];
  pinnedWorkspaces: SidebarWorkspaceEntry[];
  projectIconByProjectViewKey: ReadonlyMap<string, string | null>;
  shortcutIndexByWorkspaceKey: Map<string, number>;
  showShortcutBadges: boolean;
  onWorkspacePress?: () => void;
  hostBadgeByServerId: ReadonlyMap<string, HostBadgeModel>;
  supportsPinningByServerId: ReadonlyMap<string, boolean>;
  onToggleWorkspacePin: ToggleSidebarWorkspacePin;
  listHeaderComponent?: ReactNode;
  /** Swaps the group list for the label filter's empty state. Never the header above it. */
  sidebarFilterEmpty?: boolean;
  parentGestureRef?: MutableRefObject<GestureType | undefined>;
  /** Native pull-to-refresh control for the scroll view. */
  refreshControl?: ReactElement<RefreshControlProps>;
}

export function SidebarStatusWorkspaceList({
  groups,
  pinnedWorkspaces,
  projectIconByProjectViewKey,
  shortcutIndexByWorkspaceKey,
  showShortcutBadges,
  onWorkspacePress,
  hostBadgeByServerId,
  supportsPinningByServerId,
  onToggleWorkspacePin,
  listHeaderComponent,
  sidebarFilterEmpty = false,
  parentGestureRef,
  refreshControl,
}: StatusWorkspaceListProps) {
  const collapsedWorkspaceGroupKeys = useSidebarCollapsedSectionsStore(
    (state) => state.collapsedWorkspaceGroupKeys,
  );
  const pinnedCollapsed = useSidebarCollapsedSectionsStore((state) => state.collapsedPinned);
  const togglePinnedCollapsed = useSidebarCollapsedSectionsStore(
    (state) => state.togglePinnedCollapsed,
  );
  const {
    visibleItems: visiblePinnedWorkspaces,
    expanded: pinnedWorkspacesExpanded,
    canToggle: canTogglePinnedWorkspaces,
    toggleExpanded: togglePinnedWorkspacesExpanded,
  } = useLimitedSidebarGroup(pinnedWorkspaces);

  const statusShortcutIndex = showShortcutBadges
    ? shortcutIndexByWorkspaceKey
    : EMPTY_SHORTCUT_INDEX;
  const renderPinnedWorkspace = useCallback(
    (workspace: SidebarWorkspaceEntry) => (
      <StatusWorkspaceRow
        key={workspace.workspaceKey}
        workspace={workspace}
        {...buildStatusRowProjectPresentation({
          workspace,
          projectIconByProjectViewKey,
          hostBadgeByServerId,
        })}
        inStatusGroup={false}
        shortcutNumber={statusShortcutIndex.get(workspace.workspaceKey) ?? null}
        showShortcutBadge={showShortcutBadges}
        canPin={supportsPinningByServerId.get(workspace.serverId) === true}
        onToggleWorkspacePin={onToggleWorkspacePin}
        onWorkspacePress={onWorkspacePress}
      />
    ),
    [
      hostBadgeByServerId,
      onToggleWorkspacePin,
      onWorkspacePress,
      projectIconByProjectViewKey,
      showShortcutBadges,
      statusShortcutIndex,
      supportsPinningByServerId,
    ],
  );
  const content = (
    <>
      {pinnedWorkspaces.length > 0 ? (
        <View style={styles.pinnedSection} testID="sidebar-pinned-section">
          <PinnedSectionHeader collapsed={pinnedCollapsed} onToggle={togglePinnedCollapsed} />
          {pinnedCollapsed ? null : (
            <>
              <View testID="sidebar-pinned-list">
                {visiblePinnedWorkspaces.map(renderPinnedWorkspace)}
              </View>
              {canTogglePinnedWorkspaces ? (
                <SidebarGroupToggleRow
                  expanded={pinnedWorkspacesExpanded}
                  onPress={togglePinnedWorkspacesExpanded}
                  testID="sidebar-pinned-show-more"
                />
              ) : null}
            </>
          )}
        </View>
      ) : null}
      {listHeaderComponent}
      {sidebarFilterEmpty ? (
        <SidebarFilterEmptyState />
      ) : (
        <StatusGroupList
          groups={groups}
          collapsedWorkspaceGroupKeys={collapsedWorkspaceGroupKeys}
          projectIconByProjectViewKey={projectIconByProjectViewKey}
          shortcutIndex={statusShortcutIndex}
          showShortcutBadges={showShortcutBadges}
          onWorkspacePress={onWorkspacePress}
          hostBadgeByServerId={hostBadgeByServerId}
          supportsPinningByServerId={supportsPinningByServerId}
          onToggleWorkspacePin={onToggleWorkspacePin}
        />
      )}
    </>
  );

  return (
    <View style={styles.container}>
      {platformIsNative ? (
        <GestureScrollView
          simultaneousHandlers={parentGestureRef}
          style={styles.list}
          contentContainerStyle={styles.listContent}
          showsVerticalScrollIndicator={false}
          refreshControl={refreshControl}
          testID="sidebar-status-list-scroll"
        >
          {content}
        </GestureScrollView>
      ) : (
        <ScrollView
          style={styles.list}
          contentContainerStyle={styles.listContent}
          showsVerticalScrollIndicator={false}
          testID="sidebar-status-list-scroll"
        >
          {content}
        </ScrollView>
      )}
    </View>
  );
}

function StatusGroupList({
  groups,
  collapsedWorkspaceGroupKeys,
  projectIconByProjectViewKey,
  shortcutIndex,
  showShortcutBadges,
  onWorkspacePress,
  hostBadgeByServerId,
  supportsPinningByServerId,
  onToggleWorkspacePin,
}: {
  groups: SidebarWorkspaceGroup[];
  collapsedWorkspaceGroupKeys: ReadonlySet<string>;
  projectIconByProjectViewKey: ReadonlyMap<string, string | null>;
  shortcutIndex: Map<string, number>;
  showShortcutBadges: boolean;
  onWorkspacePress?: () => void;
  hostBadgeByServerId: ReadonlyMap<string, HostBadgeModel>;
  supportsPinningByServerId: ReadonlyMap<string, boolean>;
  onToggleWorkspacePin: ToggleSidebarWorkspacePin;
}) {
  return (
    <>
      {groups.map((group) => (
        <StatusGroupRows
          key={group.key}
          group={group}
          collapsed={collapsedWorkspaceGroupKeys.has(group.key)}
          projectIconByProjectViewKey={projectIconByProjectViewKey}
          shortcutIndex={shortcutIndex}
          showShortcutBadges={showShortcutBadges}
          onWorkspacePress={onWorkspacePress}
          hostBadgeByServerId={hostBadgeByServerId}
          supportsPinningByServerId={supportsPinningByServerId}
          onToggleWorkspacePin={onToggleWorkspacePin}
        />
      ))}
    </>
  );
}

function StatusGroupRows({
  group,
  collapsed,
  projectIconByProjectViewKey,
  shortcutIndex,
  showShortcutBadges,
  onWorkspacePress,
  hostBadgeByServerId,
  supportsPinningByServerId,
  onToggleWorkspacePin,
}: {
  group: SidebarWorkspaceGroup;
  collapsed: boolean;
  projectIconByProjectViewKey: ReadonlyMap<string, string | null>;
  shortcutIndex: Map<string, number>;
  showShortcutBadges: boolean;
  onWorkspacePress?: () => void;
  hostBadgeByServerId: ReadonlyMap<string, HostBadgeModel>;
  supportsPinningByServerId: ReadonlyMap<string, boolean>;
  onToggleWorkspacePin: ToggleSidebarWorkspacePin;
}) {
  const {
    visibleItems: visibleWorkspaces,
    expanded: workspacesExpanded,
    canToggle: canToggleWorkspaces,
    toggleExpanded: toggleWorkspacesExpanded,
  } = useLimitedSidebarGroup(group.rows);

  return (
    <View style={collapsed ? undefined : styles.statusGroupBlockExpanded}>
      <StatusGroupHeader group={group} collapsed={collapsed} />
      {!collapsed ? (
        <View
          style={styles.statusWorkspaceListContainer}
          testID={`sidebar-status-group-rows-${group.key}`}
        >
          {visibleWorkspaces.map((workspace) => (
            <StatusWorkspaceRow
              key={workspace.workspaceKey}
              workspace={workspace}
              {...buildStatusRowProjectPresentation({
                workspace,
                projectIconByProjectViewKey,
                hostBadgeByServerId,
              })}
              shortcutNumber={shortcutIndex.get(workspace.workspaceKey) ?? null}
              showShortcutBadge={showShortcutBadges}
              canPin={supportsPinningByServerId.get(workspace.serverId) === true}
              onToggleWorkspacePin={onToggleWorkspacePin}
              onWorkspacePress={onWorkspacePress}
            />
          ))}
          {canToggleWorkspaces ? (
            <SidebarGroupToggleRow
              expanded={workspacesExpanded}
              onPress={toggleWorkspacesExpanded}
              indented
              testID={`sidebar-status-group-show-more-${group.key}`}
            />
          ) : null}
        </View>
      ) : null}
    </View>
  );
}

interface StatusRowProjectPresentation {
  hostBadge: HostBadgeModel | null;
  projectName: string;
  projectIconDataUri: string | null;
}

function buildStatusRowProjectPresentation({
  workspace,
  projectIconByProjectViewKey,
  hostBadgeByServerId,
}: {
  workspace: SidebarWorkspaceEntry;
  projectIconByProjectViewKey: ReadonlyMap<string, string | null>;
  hostBadgeByServerId: ReadonlyMap<string, HostBadgeModel>;
}): StatusRowProjectPresentation {
  return {
    hostBadge: hostBadgeByServerId.get(workspace.serverId) ?? null,
    projectName: workspace.projectName,
    projectIconDataUri: projectIconByProjectViewKey.get(workspace.projectViewKey) ?? null,
  };
}

function StatusGroupHeader({
  group,
  collapsed,
}: {
  group: SidebarWorkspaceGroup;
  collapsed: boolean;
}) {
  const { t } = useTranslation();
  const { isHovered, hoverHandlers } = useSidebarRowHover();
  const toggleWorkspaceGroupCollapsed = useSidebarCollapsedSectionsStore(
    (state) => state.toggleWorkspaceGroupCollapsed,
  );
  const handlePress = useCallback(() => {
    toggleWorkspaceGroupCollapsed(group.key);
  }, [group.key, toggleWorkspaceGroupCollapsed]);
  const rowStyle = useCallback(
    ({ pressed }: PressableStateCallbackType) => [
      styles.statusGroupRow,
      isHovered && styles.statusGroupRowHovered,
      pressed && styles.statusGroupRowPressed,
    ],
    [isHovered],
  );
  const accessibilityState = useMemo(() => ({ expanded: !collapsed }), [collapsed]);

  return (
    <View {...hoverHandlers}>
      <Pressable
        accessibilityRole={platformIsWeb ? undefined : "button"}
        accessibilityLabel={t("sidebar.statusGroup.accessibilityLabel", { label: group.label })}
        accessibilityState={accessibilityState}
        style={rowStyle}
        onPress={handlePress}
        testID={`sidebar-status-group-${group.key}`}
      >
        <View style={styles.statusGroupRowLeft}>
          <View style={styles.statusGroupLeadingVisualSlot}>
            <StatusGroupLeadingVisual
              leading={group.leading}
              collapsed={collapsed}
              showChevron={isHovered}
            />
          </View>
          <View style={styles.statusGroupTitleGroup}>
            <Text style={styles.statusGroupTitle} numberOfLines={1}>
              {group.label}
            </Text>
          </View>
        </View>
      </Pressable>
    </View>
  );
}

function StatusGroupLeadingVisual({
  leading,
  collapsed,
  showChevron,
}: {
  leading: SidebarWorkspaceGroup["leading"];
  collapsed: boolean;
  showChevron: boolean;
}) {
  if (!showChevron) {
    return <StatusGroupIcon bucket={leading.bucket} />;
  }
  if (collapsed) {
    return <ThemedChevronRight size={14} uniProps={foregroundMutedColorMapping} />;
  }
  return <ThemedChevronDown size={14} uniProps={foregroundMutedColorMapping} />;
}

function StatusGroupIcon({ bucket }: { bucket: StatusBucket }) {
  switch (bucket) {
    case "needs_input":
      return <ThemedCircleAlert size={14} uniProps={needsInputColorMapping} />;
    case "failed":
      return <ThemedCircleX size={14} uniProps={failedColorMapping} />;
    case "attention":
      return <ThemedCircleCheck size={14} uniProps={attentionColorMapping} />;
    case "running":
      return <ThemedCircleDot size={14} uniProps={runningColorMapping} />;
    case "done":
      return <ThemedCircleCheck size={14} uniProps={foregroundMutedColorMapping} />;
  }
}

const StatusWorkspaceRow = memo(function StatusWorkspaceRow({
  workspace,
  hostBadge,
  projectName,
  projectIconDataUri,
  shortcutNumber,
  showShortcutBadge,
  canPin,
  onToggleWorkspacePin,
  inStatusGroup = true,
  onWorkspacePress,
}: {
  workspace: SidebarWorkspaceEntry;
  hostBadge: HostBadgeModel | null;
  projectName: string;
  projectIconDataUri: string | null;
  shortcutNumber: number | null;
  showShortcutBadge: boolean;
  canPin: boolean;
  onToggleWorkspacePin: ToggleSidebarWorkspacePin;
  /**
   * Whether the row sits under a status header, which is what it indents from. Pinned rows
   * are a flat list under their own header and sit flush.
   */
  inStatusGroup?: boolean;
  onWorkspacePress?: () => void;
}) {
  const activeWorkspaceSelection = useActiveWorkspaceSelection();
  const selected =
    activeWorkspaceSelection?.serverId === workspace.serverId &&
    activeWorkspaceSelection?.workspaceId === workspace.workspaceId;

  const handlePress = useCallback(() => {
    if (!workspace.serverId) return;
    onWorkspacePress?.();
    navigateToWorkspace({ serverId: workspace.serverId, workspaceId: workspace.workspaceId });
  }, [onWorkspacePress, workspace.serverId, workspace.workspaceId]);

  const { isArchiving, actions, renameModal, shareSheet } = useSidebarWorkspaceRowActions({
    workspace,
    selected,
    canCopyBranchName: workspace.projectKind === "git",
    canPin,
    onToggleWorkspacePin,
  });

  const isCompact = useIsCompactFormFactor();
  const isTouchPlatform = platformIsNative || isCompact;
  const showSelection = selected && !isCompact;
  const {
    isPressed,
    isHighlighted,
    onHighlightChange,
    onPressIn: handlePressIn,
    onPressOut: handlePressOut,
  } = useSidebarRowPress();
  const trailing = useSidebarWorkspaceTrailing();
  const isDesktop = !isTouchPlatform;
  const serviceSummary = isDesktop ? selectWorkspaceServiceSummary(workspace.scripts) : null;

  const accessibilityState = useMemo(() => ({ selected }), [selected]);

  return (
    <>
      <SidebarWorkspaceRowFrame workspace={workspace}>
        {({ isHovered, contextMenuOpen, onContextMenuOpenChange, hoverHandlers }) => {
          const showShortcut = showShortcutBadge && shortcutNumber !== null;
          const {
            trailingPresentation,
            showKebab: showKebabInSlot,
            showScrim,
            reserveSlotWidth,
          } = resolveTrailingActionVisibility({
            workspace,
            trailing,
            isHovered,
            isTouchPlatform,
            showShortcut,
          });
          const workspaceRowStyle = getStatusWorkspaceRowStyle({
            isPressed,
            selected: showSelection,
            isHovered,
            inStatusGroup,
          });
          const backdrop = getSidebarRowBackdrop({
            isPressed: isHighlighted,
            selected: showSelection,
            isHovered,
          });
          return (
            <View style={styles.workspaceRowContainer} {...hoverHandlers}>
              <SidebarWorkspaceContextMenu
                {...actions}
                contextMenuOpen={contextMenuOpen}
                onContextMenuOpenChange={onContextMenuOpenChange}
                workspace={workspace}
                leadingProjectName={projectName}
                hostBadgeLabel={hostBadge?.label}
                serviceSummary={serviceSummary}
                workspaceKey={workspace.workspaceKey}
                disabled={isArchiving}
                accessibilityRole="button"
                accessibilityState={accessibilityState}
                style={workspaceRowStyle}
                highlightStyle={styles.workspaceRowPressed}
                onHighlightChange={onHighlightChange}
                onPressIn={handlePressIn}
                onPressOut={handlePressOut}
                onPress={handlePress}
                testID={`sidebar-workspace-row-${workspace.workspaceKey}`}
              >
                <SidebarWorkspaceRowContent
                  workspace={workspace}
                  hostBadge={hostBadge}
                  leadingProjectName={projectName}
                  leadingProjectIconDataUri={projectIconDataUri}
                  serviceSummary={serviceSummary}
                  backdrop={backdrop}
                  isHovered={isHovered}
                  isLoading={isArchiving}
                  shortcutNumber={shortcutNumber}
                  showShortcutBadge={showShortcutBadge}
                >
                  <SidebarWorkspaceActionSlot
                    workspace={workspace}
                    backdrop={backdrop}
                    trailing={trailing}
                    trailingPresentation={trailingPresentation}
                    showKebab={showKebabInSlot}
                    showScrim={showScrim}
                    reserveSlotWidth={reserveSlotWidth}
                    actions={actions}
                  />
                </SidebarWorkspaceRowContent>
              </SidebarWorkspaceContextMenu>
            </View>
          );
        }}
      </SidebarWorkspaceRowFrame>
      {shareSheet}
      {renameModal}
    </>
  );
});

function getStatusWorkspaceRowStyle({
  isPressed,
  selected,
  isHovered,
  inStatusGroup,
}: {
  isPressed: boolean;
  selected: boolean;
  isHovered: boolean;
  inStatusGroup: boolean;
}) {
  return [
    styles.workspaceRow,
    inStatusGroup && sidebarWorkspaceRowStyles.rowIndented,
    isHovered && styles.workspaceRowHovered,
    selected && styles.sidebarRowSelected,
    isPressed && styles.workspaceRowPressed,
  ];
}

const styles = StyleSheet.create((theme) => ({
  container: {
    flex: 1,
  },
  list: {
    flex: 1,
  },
  listContent: {
    paddingHorizontal: theme.spacing[2],
    // Keep status mode's Pinned/Workspaces boundary identical to project mode.
    paddingTop: 2,
    paddingBottom: theme.spacing[4],
  },
  pinnedSection: {
    marginBottom: theme.spacing[1],
  },
  // Matches `projectBlockExpanded` in sidebar-workspace-list.tsx. See the note there.
  statusGroupBlockExpanded: {
    paddingBottom: theme.spacing[3],
  },
  statusWorkspaceListContainer: {},
  statusGroupRow: {
    minHeight: 36,
    paddingVertical: theme.spacing[2],
    paddingHorizontal: theme.spacing[2],
    borderRadius: theme.borderRadius.lg,
    marginBottom: theme.spacing[2],
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: theme.spacing[2],
    userSelect: "none",
  },
  statusGroupRowHovered: {
    backgroundColor: theme.colors.surfaceSidebarHover,
  },
  statusGroupRowPressed: {
    backgroundColor: theme.colors.surface2,
  },
  statusGroupRowLeft: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
    flex: 1,
    minWidth: 0,
  },
  statusGroupLeadingVisualSlot: {
    position: "relative",
    width: theme.iconSize.md,
    height: theme.iconSize.md,
    flexShrink: 0,
    alignItems: "center",
    justifyContent: "center",
  },
  statusGroupTitleGroup: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[1],
    flex: 1,
    minWidth: 0,
  },
  statusGroupTitle: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.base,
    fontWeight: "400",
    minWidth: 0,
    flexShrink: 1,
  },
  workspaceRowContainer: {
    position: "relative",
  },
  workspaceRow: {
    minHeight: 36,
    marginBottom: theme.spacing[0.5],
    paddingVertical: theme.spacing[2],
    paddingLeft: theme.spacing[2],
    paddingRight: theme.spacing[3],
    borderRadius: theme.borderRadius.lg,
    flexDirection: "column",
    alignItems: "stretch",
    justifyContent: "flex-start",
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
}));

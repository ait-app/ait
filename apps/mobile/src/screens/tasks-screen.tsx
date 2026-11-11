import React, { useCallback, useDeferredValue, useEffect, useMemo, useState } from "react";
import { FlatList, Pressable, ScrollView, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { useIsFocused } from "@react-navigation/native";
import { router } from "expo-router";
import {
  Folder,
  LayoutDashboard,
  Plus,
  RefreshCw,
  Search,
  SlidersHorizontal,
  GitFork,
  PanelRight,
  ChevronsLeft,
  ChevronsRight,
  type LucideIcon,
} from "lucide-react-native";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import { MenuHeader } from "@/components/headers/menu-header";
import { HeaderToggleButton } from "@/components/headers/header-toggle-button";
import { HostFilter } from "@/components/hosts/host-filter";
import { ALL_HOSTS_OPTION_ID } from "@/components/hosts/host-picker";
import { AdaptiveModalSheet } from "@/components/adaptive-modal-sheet";
import { Button } from "@/components/ui/button";
import { SearchField } from "@/components/ui/search-field";
import { SelectField } from "@/components/ui/select-field";
import { LoadingSpinner } from "@/components/ui/loading-spinner";
import { COMPACT_LARGE_ICON_BUTTON_SIZE } from "@/components/ui/icon-button-chrome";
import { buildNewWorkspaceRoute } from "@/utils/host-routes";
import { useTaskBoard } from "@/task-board/use-task-board";
import {
  filterTaskBoard,
  TASK_LANES,
  taskKey,
  taskLane,
  type TaskLane,
  type TaskSort,
} from "@/task-board/model";
import { TaskCard } from "@/task-board/task-card";
import { TaskDetail } from "@/task-board/task-detail";
import { TaskStatusIcon } from "@/task-board/task-status";
import { useProjectIcons } from "@/projects/icons";

const MIN_LANE_WIDTH = 240;
const DETAIL_WIDTH = 348;
const COLLAPSED_LANE_WIDTH = 40;

function BoardButton({
  icon: Icon,
  label,
  onPress,
  testID,
  active = false,
  disabled = false,
}: {
  icon: LucideIcon;
  label: string;
  onPress: () => void;
  testID: string;
  active?: boolean;
  disabled?: boolean;
}) {
  const { theme } = useUnistyles();
  return (
    <HeaderToggleButton
      tooltipLabel={label}
      tooltipKeys={[]}
      tooltipSide="bottom"
      onPress={onPress}
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityState={{ selected: active }}
      aria-pressed={active}
      disabled={disabled}
      testID={testID}
      style={active ? styles.activeControl : undefined}
    >
      <Icon size={16} color={active ? theme.colors.accent : theme.colors.foregroundMuted} />
    </HeaderToggleButton>
  );
}

export function TasksScreen() {
  const focused = useIsFocused();
  return focused ? <TasksScreenContent /> : <View style={styles.container} />;
}

export function TasksScreenContent() {
  const { t } = useTranslation();
  const { theme } = useUnistyles();
  const {
    roots,
    byKey,
    hosts,
    statuses,
    unavailableHosts,
    allUnavailable,
    isInitialLoad,
    refresh,
  } = useTaskBoard();
  const [width, setWidth] = useState(0);
  const [boardWidth, setBoardWidth] = useState(0);
  const [search, setSearch] = useState("");
  const deferredSearch = useDeferredValue(search);
  const [hostId, setHostId] = useState(ALL_HOSTS_OPTION_ID);
  const [projectKey, setProjectKey] = useState("");
  const [sort, setSort] = useState<TaskSort>("recent");
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [mobileLane, setMobileLane] = useState<TaskLane>("needs_action");
  const [collapsedLanes, setCollapsedLanes] = useState<TaskLane[]>([]);
  const [searchOpen, setSearchOpen] = useState(false);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [showChildren, setShowChildren] = useState(true);
  const selected = selectedKey ? byKey.get(selectedKey) : undefined;
  const compact = width > 0 && width < 700;
  const minimumBoardWidth =
    (TASK_LANES.length - collapsedLanes.length) * MIN_LANE_WIDTH +
    collapsedLanes.length * COLLAPSED_LANE_WIDTH +
    (TASK_LANES.length + 1) * theme.spacing[2];
  const inlineDetail = width >= minimumBoardWidth + DETAIL_WIDTH;
  const lanesWidth = compact ? boardWidth : Math.max(boardWidth, minimumBoardWidth);
  const closeDetail = useCallback(() => setSelectedKey(null), []);
  const toggleLane = useCallback(
    (lane: TaskLane) =>
      setCollapsedLanes((current) =>
        current.includes(lane) ? current.filter((value) => value !== lane) : [...current, lane],
      ),
    [],
  );
  const iconTargets = useMemo(
    () =>
      [...byKey.values()].flatMap((task) =>
        task.projectIconTarget ? [task.projectIconTarget] : [],
      ),
    [byKey],
  );
  const projectIcons = useProjectIcons({ projects: iconTargets });
  const selectHost = useCallback((id: string) => {
    setHostId(id);
    setProjectKey("");
  }, []);
  useEffect(() => {
    if (hostId !== ALL_HOSTS_OPTION_ID && !hosts.some((host) => host.serverId === hostId))
      selectHost(ALL_HOSTS_OPTION_ID);
  }, [hostId, hosts, selectHost]);
  useEffect(() => {
    if (selectedKey && !byKey.has(selectedKey)) closeDetail();
  }, [byKey, closeDetail, selectedKey]);
  const projects = useMemo(() => {
    const options = new Map<
      string,
      { id: string; value: string; label: string; description: string }
    >();
    for (const task of byKey.values()) {
      if (hostId !== ALL_HOSTS_OPTION_ID && task.agent.serverId !== hostId) continue;
      options.set(task.projectKey, {
        id: task.projectKey,
        value: task.projectKey,
        label: task.projectName,
        description: task.hostLabel,
      });
    }
    return [
      { id: "all", value: "", label: t("taskBoard.allProjects"), description: "" },
      ...[...options.values()].sort((a, b) => a.label.localeCompare(b.label)),
    ];
  }, [byKey, hostId, t]);
  const project = projects.find((option) => option.value === projectKey) ?? projects[0];
  const visible = useMemo(
    () =>
      filterTaskBoard(roots, {
        hostId: hostId === ALL_HOSTS_OPTION_ID ? null : hostId,
        projectKey: project.value || null,
        search: deferredSearch,
        sort,
      }),
    [roots, hostId, project.value, deferredSearch, sort],
  );
  const lanes = useMemo(
    () =>
      Object.fromEntries(
        TASK_LANES.map((lane) => [lane, visible.filter((task) => taskLane(task.bucket) === lane)]),
      ) as Record<TaskLane, typeof visible>,
    [visible],
  );
  const sortOptions = ["recent", "oldest"].map((value) => ({
    id: value,
    value: value as TaskSort,
    label: t(`taskBoard.sort.${value as TaskSort}`),
  }));
  const offlineHosts = unavailableHosts.filter(
    (host) => hostId === ALL_HOSTS_OPTION_ID || host.serverId === hostId,
  );
  const detail = selected ? (
    <TaskDetail
      key={selected.key}
      task={selected}
      parent={
        selected.agent.parentAgentId
          ? byKey.get(taskKey(selected.agent.serverId, selected.agent.parentAgentId))
          : undefined
      }
      showHeader={inlineDetail}
      online={statuses.get(selected.agent.serverId) === "online"}
      onSelect={setSelectedKey}
      onClose={closeDetail}
    />
  ) : null;
  return (
    <View
      style={styles.container}
      onLayout={(event) => setWidth(event.nativeEvent.layout.width)}
      testID="task-board"
    >
      <MenuHeader title={t("taskBoard.title")} />
      <View style={styles.body}>
        <View style={styles.main}>
          <View style={styles.toolbar} testID="task-board-toolbar">
            <View style={[styles.toolbarLeading, searchOpen && styles.searchExpanded]}>
              {searchOpen ? (
                <SearchField
                  value={search}
                  onChangeText={setSearch}
                  placeholder={t("taskBoard.search")}
                  clearAccessibilityLabel={t("taskBoard.clearSearch")}
                  testID="task-board-search"
                />
              ) : (
                <BoardButton
                  icon={Search}
                  label={t("taskBoard.search")}
                  onPress={() => setSearchOpen(true)}
                  testID="task-board-search-toggle"
                />
              )}
            </View>
            {!compact || !searchOpen ? (
              <Text style={styles.taskCount} numberOfLines={1}>
                {t("taskBoard.taskCount", { count: visible.length })}
              </Text>
            ) : null}
            <View style={styles.toolbarActions}>
              <BoardButton
                icon={SlidersHorizontal}
                label={t("taskBoard.filters")}
                onPress={() => setFiltersOpen(true)}
                testID="task-board-filters"
                active={hostId !== ALL_HOSTS_OPTION_ID || Boolean(projectKey) || sort !== "recent"}
              />
              <BoardButton
                icon={GitFork}
                label={t("taskBoard.showSubtasks")}
                onPress={() => setShowChildren(!showChildren)}
                testID="task-board-subtasks"
                active={showChildren}
              />
              <BoardButton
                icon={PanelRight}
                label={t("taskBoard.details")}
                onPress={() => (selected ? closeDetail() : setSelectedKey(visible[0]?.key ?? null))}
                testID="task-board-details-toggle"
                active={Boolean(selected)}
                disabled={!visible.length}
              />
              <BoardButton
                icon={Plus}
                label={t("taskBoard.newTask")}
                onPress={() => router.push(buildNewWorkspaceRoute())}
                testID="task-board-new"
              />
            </View>
          </View>
          <AdaptiveModalSheet
            visible={filtersOpen}
            onClose={() => setFiltersOpen(false)}
            header={{ title: t("taskBoard.filters") }}
            desktopMaxWidth={460}
            snapPoints={["50%", "85%"]}
            testID="task-board-filter-sheet"
          >
            <View style={styles.filterSheet}>
              <View style={styles.filterControls}>
                {hosts.length > 1 ? (
                  <HostFilter
                    hosts={hosts}
                    selectedHost={hostId}
                    onSelectHost={selectHost}
                    triggerTestID="task-board-host-filter"
                  />
                ) : null}
                <SelectField
                  field={false}
                  size="sm"
                  value={project.value}
                  selectedDisplay={project}
                  options={projects}
                  onChange={setProjectKey}
                  label={t("taskBoard.project")}
                  placeholder={t("taskBoard.allProjects")}
                  emptyText={t("taskBoard.noMatches")}
                  triggerLeading={<Folder size={14} color={theme.colors.foregroundMuted} />}
                  triggerTestID="task-board-project-filter"
                />
                <SelectField
                  field={false}
                  size="sm"
                  value={sort}
                  selectedDisplay={sortOptions.find((option) => option.value === sort)!}
                  options={sortOptions}
                  onChange={setSort}
                  label={t("taskBoard.sortLabel")}
                  placeholder={t("taskBoard.sort.recent")}
                  emptyText={t("taskBoard.noMatches")}
                  triggerTestID="task-board-sort"
                />
              </View>

              <Button
                size="sm"
                variant="ghost"
                onPress={() => {
                  selectHost(ALL_HOSTS_OPTION_ID);
                  setProjectKey("");
                  setSort("recent");
                  setSearch("");
                }}
              >
                {t("taskBoard.resetFilters")}
              </Button>
              <Button onPress={() => setFiltersOpen(false)}>
                {t("taskBoard.showTasks", { count: visible.length })}
              </Button>
            </View>
          </AdaptiveModalSheet>
          {offlineHosts.length ? (
            <View
              style={[styles.notice, compact && styles.compactPadding]}
              testID="task-board-notice"
            >
              <Text style={styles.noticeText}>
                {t("taskBoard.hostsUnavailable", {
                  hosts: offlineHosts.map((host) => host.label).join(", "),
                })}
              </Text>
              <Button variant="ghost" size="sm" leftIcon={RefreshCw} onPress={() => void refresh()}>
                {t("taskBoard.retry")}
              </Button>
            </View>
          ) : null}
          {isInitialLoad ? (
            <View style={styles.empty}>
              <LoadingSpinner size="large" color={theme.colors.foregroundMuted} />
              <Text style={styles.subtitle}>{t("taskBoard.loading")}</Text>
            </View>
          ) : allUnavailable && roots.length === 0 ? (
            <View style={styles.empty}>
              <Text style={styles.emptyTitle}>
                {t("taskBoard.hostsUnavailable", {
                  hosts: offlineHosts.map((host) => host.label).join(", "),
                })}
              </Text>
              <Button variant="outline" onPress={refresh}>
                {t("taskBoard.retry")}
              </Button>
            </View>
          ) : roots.length === 0 ? (
            <View style={styles.empty} testID="task-board-empty">
              <LayoutDashboard size={36} color={theme.colors.foregroundMuted} />
              <Text style={styles.emptyTitle}>{t("taskBoard.emptyTitle")}</Text>
              <Text style={styles.subtitle}>{t("taskBoard.emptyDescription")}</Text>
              <Button
                variant="outline"
                leftIcon={Plus}
                onPress={() => router.push(buildNewWorkspaceRoute())}
              >
                {t("taskBoard.newTask")}
              </Button>
            </View>
          ) : (
            <View style={styles.workspace}>
              <View
                style={styles.board}
                onLayout={(event) => setBoardWidth(event.nativeEvent.layout.width)}
              >
                {compact ? (
                  <ScrollView
                    horizontal
                    style={styles.tabs}
                    contentContainerStyle={styles.tabsContent}
                    showsHorizontalScrollIndicator={false}
                  >
                    {TASK_LANES.map((lane) => (
                      <Pressable
                        key={lane}
                        accessibilityRole="tab"
                        accessibilityState={{ selected: mobileLane === lane }}
                        style={[styles.tab, mobileLane === lane && styles.activeTab]}
                        onPress={() => setMobileLane(lane)}
                        testID={`task-tab-${lane}`}
                      >
                        <TaskStatusIcon
                          bucket={
                            lane === "needs_action"
                              ? "needs_input"
                              : lane === "review"
                                ? "attention"
                                : lane
                          }
                          size={10}
                        />
                        <Text style={[styles.tabText, mobileLane === lane && styles.activeTabText]}>
                          {t(`taskBoard.lanes.${lane}`)} {lanes[lane].length}
                        </Text>
                      </Pressable>
                    ))}
                  </ScrollView>
                ) : null}
                <ScrollView
                  horizontal
                  style={styles.boardScroll}
                  contentContainerStyle={[
                    styles.lanes,
                    { width: lanesWidth },
                    compact && styles.compactLanes,
                  ]}
                  showsHorizontalScrollIndicator={!compact}
                  scrollEnabled={!compact}
                >
                  {(compact ? [mobileLane] : TASK_LANES).map((lane) => (
                    <View
                      key={lane}
                      style={[
                        styles.lane,
                        compact && styles.compactLane,
                        !compact && collapsedLanes.includes(lane) && styles.collapsedLane,
                      ]}
                      testID={`task-lane-${lane}`}
                    >
                      {!compact && collapsedLanes.includes(lane) ? (
                        <Pressable
                          style={styles.collapsedHeader}
                          accessibilityRole="button"
                          accessibilityLabel={t("taskBoard.expandLane", {
                            lane: t(`taskBoard.lanes.${lane}`),
                          })}
                          accessibilityState={{ expanded: false }}
                          onPress={() => toggleLane(lane)}
                          testID={`task-expand-${lane}`}
                        >
                          <TaskStatusIcon
                            bucket={
                              lane === "needs_action"
                                ? "needs_input"
                                : lane === "review"
                                  ? "attention"
                                  : lane
                            }
                          />
                          <ChevronsRight size={14} color={theme.colors.foregroundMuted} />
                          <Text style={styles.collapsedTitle}>
                            {t(`taskBoard.lanes.${lane}`)} {lanes[lane].length}
                          </Text>
                        </Pressable>
                      ) : (
                        <>
                          {!compact ? (
                            <View style={styles.laneHeader}>
                              <TaskStatusIcon
                                bucket={
                                  lane === "needs_action"
                                    ? "needs_input"
                                    : lane === "review"
                                      ? "attention"
                                      : lane
                                }
                              />
                              <Text style={styles.laneTitle}>{t(`taskBoard.lanes.${lane}`)}</Text>
                              <Text style={styles.count}>{lanes[lane].length}</Text>
                              <View style={styles.collapseControl}>
                                <BoardButton
                                  icon={ChevronsLeft}
                                  label={t("taskBoard.collapseLane", {
                                    lane: t(`taskBoard.lanes.${lane}`),
                                  })}
                                  onPress={() => toggleLane(lane)}
                                  testID={`task-collapse-${lane}`}
                                />
                              </View>
                            </View>
                          ) : null}
                          <FlatList
                            data={lanes[lane]}
                            keyExtractor={(task) => task.key}
                            style={styles.list}
                            contentContainerStyle={styles.listContent}
                            initialNumToRender={8}
                            windowSize={5}
                            keyboardShouldPersistTaps="handled"
                            renderItem={({ item }) => (
                              <TaskCard
                                task={item}
                                selected={item.key === selectedKey}
                                online={statuses.get(item.agent.serverId) === "online"}
                                showChildren={showChildren}
                                iconDataUri={projectIcons.get(item.projectKey)}
                                onSelect={setSelectedKey}
                              />
                            )}
                            ListEmptyComponent={
                              <View style={styles.emptyLane}>
                                <Text style={styles.subtitle}>
                                  {t(
                                    visible.length ? "taskBoard.emptyLane" : "taskBoard.noMatches",
                                  )}
                                </Text>
                              </View>
                            }
                          />
                        </>
                      )}
                    </View>
                  ))}
                </ScrollView>
              </View>
            </View>
          )}
        </View>
        {inlineDetail && detail ? <View style={styles.detail}>{detail}</View> : null}
      </View>
      {!inlineDetail && selected ? (
        <AdaptiveModalSheet
          header={{ title: t("taskBoard.details") }}
          visible
          onClose={closeDetail}
          snapPoints={["85%", "95%"]}
          desktopMaxWidth={480}
          desktopHeight="85%"
          scrollable={false}
          testID="task-detail-sheet"
          bodyStyle={styles.sheetBody}
          contentStyle={styles.sheetBody}
        >
          {detail}
        </AdaptiveModalSheet>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  container: { flex: 1, minWidth: 0, minHeight: 0, backgroundColor: theme.colors.surface0 },
  body: { flex: 1, flexDirection: "row", minWidth: 0, minHeight: 0 },
  main: { flex: 1, minWidth: 0, minHeight: 0 },
  toolbar: {
    minHeight: 48,
    paddingHorizontal: theme.spacing[3],
    gap: theme.spacing[2],
    flexDirection: "row",
    alignItems: "center",
  },
  toolbarLeading: { flex: 1, minWidth: 0, flexDirection: "row", alignItems: "center" },
  searchExpanded: { flexGrow: 2 },
  toolbarActions: {
    flex: 1,
    minWidth: 4 * COMPACT_LARGE_ICON_BUTTON_SIZE + 3 * theme.spacing[1],
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "flex-end",
    gap: theme.spacing[1],
  },
  taskCount: {
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.semibold,
    color: theme.colors.foreground,
  },
  activeControl: { backgroundColor: theme.colors.surface2 },
  filterSheet: { padding: theme.spacing[4], gap: theme.spacing[4] },
  subtitle: { color: theme.colors.foregroundMuted, fontSize: theme.fontSize.sm, lineHeight: 18 },
  filterControls: {
    flexDirection: "row",
    flexWrap: "wrap",
    gap: theme.spacing[2],
    alignItems: "center",
  },
  compactPadding: { paddingHorizontal: theme.spacing[4] },
  notice: {
    paddingHorizontal: theme.spacing[4],
    paddingBottom: theme.spacing[2],
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
  },
  noticeText: {
    flex: 1,
    color: theme.colors.statusWarning,
    fontSize: theme.fontSize.sm,
    lineHeight: 19,
  },
  empty: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
    padding: theme.spacing[6],
    gap: theme.spacing[4],
  },
  emptyTitle: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.xl,
    fontWeight: theme.fontWeight.semibold,
  },
  workspace: {
    flex: 1,
    minWidth: 0,
    minHeight: 0,
    flexDirection: "row",
    borderTopWidth: 1,
    borderTopColor: theme.colors.border,
  },
  board: { flex: 1, minWidth: 0, minHeight: 0 },
  boardScroll: { flex: 1 },
  lanes: { flexDirection: "row", padding: theme.spacing[2], gap: theme.spacing[2] },
  lane: {
    flex: 1,
    minWidth: 0,
    backgroundColor: theme.colors.surface1,
    borderRadius: theme.borderRadius.lg,
  },
  laneHeader: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
    padding: theme.spacing[3],
  },
  collapseControl: { marginLeft: "auto" },
  collapsedLane: { flex: 0, width: COLLAPSED_LANE_WIDTH, minWidth: COLLAPSED_LANE_WIDTH },
  collapsedHeader: {
    height: 190,
    alignItems: "center",
    paddingTop: theme.spacing[3],
    gap: theme.spacing[2],
  },
  collapsedTitle: {
    position: "absolute",
    top: 110,
    left: -50,
    width: 140,
    textAlign: "center",
    transform: [{ rotate: "90deg" }],
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
  },
  laneTitle: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.semibold,
  },
  count: {
    color: theme.colors.foregroundMuted,
    paddingHorizontal: theme.spacing[1],
    borderRadius: theme.borderRadius.base,
    fontSize: theme.fontSize.sm,
  },
  list: { flex: 1 },
  listContent: { paddingHorizontal: theme.spacing[2], paddingBottom: theme.spacing[3] },
  emptyLane: {
    padding: theme.spacing[4],
    alignItems: "center",
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderStyle: "dashed",
    borderRadius: theme.borderRadius.lg,
  },
  detail: { width: DETAIL_WIDTH, borderLeftWidth: 1, borderLeftColor: theme.colors.border },
  tabs: { flexGrow: 0, flexShrink: 0, maxHeight: 52 },
  tabsContent: { paddingHorizontal: theme.spacing[4], alignItems: "center", gap: theme.spacing[2] },
  tab: {
    paddingHorizontal: theme.spacing[3],
    paddingVertical: theme.spacing[3],
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[1],
    borderBottomWidth: 2,
    borderBottomColor: "transparent",
  },
  activeTab: { borderBottomColor: theme.colors.accent },
  tabText: { color: theme.colors.foregroundMuted, fontSize: theme.fontSize.sm },
  activeTabText: { color: theme.colors.accent, fontWeight: theme.fontWeight.semibold },
  compactLanes: { padding: theme.spacing[3] },
  compactLane: { minWidth: 0 },
  sheetBody: { padding: 0, flex: 1, minHeight: 0 },
}));

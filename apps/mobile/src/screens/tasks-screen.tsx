import React, { useCallback, useDeferredValue, useEffect, useMemo, useState } from "react";
import { FlatList, Pressable, ScrollView, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { useIsFocused } from "@react-navigation/native";
import { router } from "expo-router";
import { Folder, LayoutDashboard, Plus, RefreshCw } from "lucide-react-native";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import { MenuHeader } from "@/components/headers/menu-header";
import { HostFilter } from "@/components/hosts/host-filter";
import { ALL_HOSTS_OPTION_ID } from "@/components/hosts/host-picker";
import { AdaptiveModalSheet } from "@/components/adaptive-modal-sheet";
import { Button } from "@/components/ui/button";
import { SearchField } from "@/components/ui/search-field";
import { SelectField } from "@/components/ui/select-field";
import { LoadingSpinner } from "@/components/ui/loading-spinner";
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
  const [search, setSearch] = useState("");
  const deferredSearch = useDeferredValue(search);
  const [hostId, setHostId] = useState(ALL_HOSTS_OPTION_ID);
  const [projectKey, setProjectKey] = useState("");
  const [sort, setSort] = useState<TaskSort>("recent");
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [mobileLane, setMobileLane] = useState<TaskLane>("needs_action");
  const selected = selectedKey ? byKey.get(selectedKey) : undefined;
  const compact = width > 0 && width < 700;
  const inlineDetail = width >= 1320;
  const closeDetail = useCallback(() => setSelectedKey(null), []);
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
          <View style={[styles.intro, compact && styles.compactPadding]}>
            <View style={styles.introText}>
              <Text style={styles.title}>{t("taskBoard.title")}</Text>
              <Text style={styles.subtitle}>
                {t("taskBoard.summary", {
                  count: visible.length,
                  needsAction: lanes.needs_action.length,
                })}
              </Text>
            </View>
            <Button
              variant="outline"
              size="sm"
              leftIcon={Plus}
              onPress={() => router.push(buildNewWorkspaceRoute())}
              testID="task-board-new"
            >
              {t("taskBoard.newTask")}
            </Button>
          </View>
          <View style={[styles.filters, compact && styles.compactPadding]}>
            <View style={styles.search}>
              <SearchField
                value={search}
                onChangeText={setSearch}
                placeholder={t("taskBoard.search")}
                clearAccessibilityLabel={t("taskBoard.clearSearch")}
                testID="task-board-search"
              />
            </View>
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
          </View>
          {offlineHosts.length ? (
            <View style={[styles.notice, compact && styles.compactPadding]}>
              <Text style={styles.noticeText}>{t("taskBoard.loadFailed")}</Text>
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
              <View style={styles.board}>
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
                  contentContainerStyle={[styles.lanes, compact && styles.compactLanes]}
                  showsHorizontalScrollIndicator={!compact}
                  scrollEnabled={!compact}
                >
                  {(compact ? [mobileLane] : TASK_LANES).map((lane) => (
                    <View
                      key={lane}
                      style={[styles.lane, compact && styles.compactLane]}
                      testID={`task-lane-${lane}`}
                    >
                      <View style={styles.laneHeader}>
                        <View
                          style={[
                            styles.dot,
                            {
                              backgroundColor:
                                lane === "needs_action"
                                  ? theme.colors.statusWarning
                                  : lane === "running"
                                    ? theme.colors.statusDotRunning
                                    : theme.colors.statusSuccess,
                            },
                          ]}
                        />
                        <Text style={styles.laneTitle}>{t(`taskBoard.lanes.${lane}`)}</Text>
                        <Text style={styles.count}>{lanes[lane].length}</Text>
                      </View>
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
                            onSelect={setSelectedKey}
                          />
                        )}
                        ListEmptyComponent={
                          <View style={styles.emptyLane}>
                            <Text style={styles.subtitle}>
                              {t(visible.length ? "taskBoard.emptyLane" : "taskBoard.noMatches")}
                            </Text>
                          </View>
                        }
                      />
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
  container: { flex: 1, minHeight: 0, backgroundColor: theme.colors.surface0 },
  body: { flex: 1, flexDirection: "row", minHeight: 0 },
  main: { flex: 1, minWidth: 0, minHeight: 0 },
  intro: {
    paddingHorizontal: theme.spacing[6],
    paddingTop: theme.spacing[6],
    paddingBottom: theme.spacing[4],
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: theme.spacing[3],
  },
  introText: { flex: 1, gap: theme.spacing[2] },
  title: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize["4xl"],
    fontWeight: theme.fontWeight.semibold,
  },
  subtitle: { color: theme.colors.foregroundMuted, fontSize: theme.fontSize.sm, lineHeight: 20 },
  filters: {
    paddingHorizontal: theme.spacing[6],
    paddingBottom: theme.spacing[4],
    flexDirection: "row",
    flexWrap: "wrap",
    gap: theme.spacing[3],
    alignItems: "center",
  },
  search: { minWidth: 220, maxWidth: 420, flexGrow: 1, flexDirection: "row" },
  filterControls: {
    flexDirection: "row",
    flexWrap: "wrap",
    gap: theme.spacing[2],
    alignItems: "center",
  },
  compactPadding: { paddingHorizontal: theme.spacing[4] },
  notice: {
    paddingHorizontal: theme.spacing[6],
    paddingBottom: theme.spacing[3],
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
    minHeight: 0,
    flexDirection: "row",
    borderTopWidth: 1,
    borderTopColor: theme.colors.border,
  },
  board: { flex: 1, minWidth: 0, minHeight: 0 },
  boardScroll: { flex: 1 },
  lanes: { flexGrow: 1, flexDirection: "row", padding: theme.spacing[4], gap: theme.spacing[4] },
  lane: {
    flex: 1,
    minWidth: 240,
    backgroundColor: theme.colors.surface1,
    borderRadius: theme.borderRadius.lg,
  },
  laneHeader: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
    padding: theme.spacing[3],
    paddingVertical: theme.spacing[4],
  },
  dot: { width: 7, height: 7, borderRadius: 4 },
  laneTitle: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.lg,
    fontWeight: theme.fontWeight.semibold,
  },
  count: {
    color: theme.colors.foregroundMuted,
    backgroundColor: theme.colors.surface2,
    paddingHorizontal: theme.spacing[2],
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
  detail: { width: 348, borderLeftWidth: 1, borderLeftColor: theme.colors.border },
  tabs: { flexGrow: 0, flexShrink: 0, maxHeight: 52 },
  tabsContent: { paddingHorizontal: theme.spacing[4], alignItems: "center", gap: theme.spacing[2] },
  tab: {
    paddingHorizontal: theme.spacing[3],
    paddingVertical: theme.spacing[3],
    borderBottomWidth: 2,
    borderBottomColor: "transparent",
  },
  activeTab: { borderBottomColor: theme.colors.accent },
  tabText: { color: theme.colors.foregroundMuted, fontSize: theme.fontSize.base },
  activeTabText: { color: theme.colors.accent, fontWeight: theme.fontWeight.semibold },
  compactLanes: { padding: theme.spacing[3] },
  compactLane: { minWidth: 0 },
  sheetBody: { padding: 0, flex: 1, minHeight: 0 },
}));

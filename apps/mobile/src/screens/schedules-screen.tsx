import { getHostGroupServerIds, groupHosts } from "@/hosts/host-groups";
import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  useSyncExternalStore,
  type ReactElement,
} from "react";
import { useTranslation } from "react-i18next";
import { ScrollView, View } from "react-native";
import { useIsFocused } from "@react-navigation/native";
import { Plus } from "lucide-react-native";
import { StyleSheet } from "react-native-unistyles";
import { MenuHeader } from "@/components/headers/menu-header";
import { SettingsCard, SettingsRow, SettingsSection } from "@/components/settings";
import { HostFilter } from "@/components/hosts/host-filter";
import { ALL_HOSTS_OPTION_ID } from "@/components/hosts/host-picker-constants";
import { ScheduleFormSheet } from "@/components/schedules/schedule-form-sheet";
import { SchedulesTable, type ScheduleRowView } from "@/components/schedules/schedules-table";
import { Button } from "@/components/ui/button";
import { LoadingSpinner } from "@/components/ui/loading-spinner";
import { useAggregatedAgents } from "@/hooks/use-aggregated-agents";
import { useProjects } from "@/hooks/use-projects";
import {
  useSchedules,
  type AggregateLoadState,
  type AggregatedSchedule,
} from "@/hooks/use-schedules";
import { getHostRuntimeStore, useHosts } from "@/runtime/host-runtime";
import {
  resolveSchedule,
  scheduleEndTime,
  type ScheduleTargetAgent,
} from "@/schedules/schedule-derivation";
import { resolveSchedulesScreenBodyState } from "./schedules-screen-state";
import { useScheduleWorkspaces } from "@/schedules/use-schedule-workspaces";
import {
  buildProjectNameByCwd,
  buildScheduleProjectTargets,
} from "@/schedules/schedule-project-targets";
import type { ScheduleSummary } from "@ait/protocol/schedule/types";

type FormState =
  | { mode: "closed" }
  | { mode: "create" }
  | { mode: "edit"; serverId: string; schedule: ScheduleSummary };

const EMPTY_SCHEDULES: AggregatedSchedule[] = [];
const ENDED_PAGE_SIZE = 10;

export function SchedulesScreen(): ReactElement {
  const isFocused = useIsFocused();

  if (!isFocused) {
    return <View style={styles.container} />;
  }

  return <SchedulesScreenContent />;
}

function SchedulesScreenContent(): ReactElement {
  const { t } = useTranslation();
  const { loadState, isError } = useSchedules();
  const schedules = loadState.status === "loaded" ? loadState.data : EMPTY_SCHEDULES;
  const { agents } = useAggregatedAgents({ includeArchived: true });
  const { projects } = useProjects();
  const workspaces = useScheduleWorkspaces();
  const workspaceNameByKey = useMemo(
    () =>
      new Map(
        workspaces.map((workspace) => [`${workspace.serverId}:${workspace.id}`, workspace.name]),
      ),
    [workspaces],
  );
  const hosts = useHosts();
  const runtime = getHostRuntimeStore();
  const runtimeVersion = useSyncExternalStore(
    (onStoreChange) => runtime.subscribeAll(onStoreChange),
    () => runtime.getVersion(),
    () => runtime.getVersion(),
  );

  // Per-host agent-directory readiness from the runtime, not the aggregate agent
  // flag: the aggregate `isInitialLoad` flips false as soon as *any* host has
  // agents, so a still-loading host would falsely mark its agent-target
  // schedules "gone". `hasEverLoadedAgentDirectory` is true only once that
  // host's directory has loaded at least once.
  const agentDirReadyHosts = useMemo(() => {
    void runtimeVersion;
    const ready = new Set<string>();
    for (const host of hosts) {
      if (runtime.getSnapshot(host.serverId)?.hasEverLoadedAgentDirectory) {
        ready.add(host.serverId);
      }
    }
    return ready;
  }, [hosts, runtime, runtimeVersion]);

  const [form, setForm] = useState<FormState>({ mode: "closed" });
  const [selectedHost, setSelectedHost] = useState(ALL_HOSTS_OPTION_ID);

  useEffect(() => {
    if (
      selectedHost !== ALL_HOSTS_OPTION_ID &&
      !hosts.some((host) => host.serverId === selectedHost)
    ) {
      setSelectedHost(ALL_HOSTS_OPTION_ID);
    }
  }, [hosts, selectedHost]);

  const openCreate = useCallback(() => setForm({ mode: "create" }), []);
  const openEdit = useCallback((schedule: AggregatedSchedule) => {
    setForm({ mode: "edit", serverId: schedule.serverId, schedule });
  }, []);
  const closeForm = useCallback(() => setForm({ mode: "closed" }), []);

  const agentsByKey = useMemo(() => {
    const map = new Map<string, ScheduleTargetAgent>();
    for (const agent of agents) {
      map.set(`${agent.serverId}:${agent.id}`, { title: agent.title, provider: agent.provider });
    }
    return map;
  }, [agents]);

  const projectNameByCwd = useMemo(
    () => buildProjectNameByCwd(buildScheduleProjectTargets(projects)),
    [projects],
  );

  // Resolve each schedule once before filtering hosts and grouping by status.
  const resolvedRows = useMemo(() => {
    const now = Date.now();
    return schedules.map((schedule) => ({
      schedule,
      resolved: resolveSchedule(
        {
          schedule,
          serverId: schedule.serverId,
          now,
          agentsByKey,
          projectNameByCwd,
          workspaceNameByKey,
          agentDataLoaded: agentDirReadyHosts.has(schedule.serverId),
        },
        t,
      ),
    }));
  }, [schedules, agentsByKey, projectNameByCwd, workspaceNameByKey, agentDirReadyHosts, t]);

  const { runnableRows, endedRows } = useMemo(() => {
    const singleHost = groupHosts(hosts).length <= 1 || selectedHost !== ALL_HOSTS_OPTION_ID;
    const selectedServerIds = new Set(getHostGroupServerIds(hosts, selectedHost));
    const runnableRows: ScheduleRowView[] = [];
    const endedRows: ScheduleRowView[] = [];
    for (const { schedule, resolved } of resolvedRows) {
      if (selectedHost !== ALL_HOSTS_OPTION_ID && !selectedServerIds.has(schedule.serverId)) {
        continue;
      }
      const row: ScheduleRowView = {
        schedule,
        targetLabel: resolved.target.label,
        provider: resolved.target.provider,
        state: resolved.state,
        serverName: schedule.serverName,
        singleHost,
      };
      (resolved.bucket === "runnable" ? runnableRows : endedRows).push(row);
    }
    runnableRows.sort(
      (a, b) => Date.parse(b.schedule.createdAt) - Date.parse(a.schedule.createdAt),
    );
    endedRows.sort(
      (a, b) =>
        scheduleEndTime(b.schedule, b.state) - scheduleEndTime(a.schedule, a.state) ||
        a.schedule.serverId.localeCompare(b.schedule.serverId) ||
        a.schedule.id.localeCompare(b.schedule.id),
    );
    return { runnableRows, endedRows };
  }, [resolvedRows, selectedHost, hosts]);

  const showLoadError = isError && loadState.status !== "loaded";
  const showHostFilter = groupHosts(hosts).length > 1;

  return (
    <View style={styles.container}>
      <MenuHeader title={t("schedules.title")} />
      <SchedulesScreenBody
        key={selectedHost}
        runnableRows={runnableRows}
        endedRows={endedRows}
        loadState={loadState}
        showLoadError={showLoadError}
        showHostFilter={showHostFilter}
        hosts={hosts}
        selectedHost={selectedHost}
        onSelectHost={setSelectedHost}
        onCreate={openCreate}
        onEdit={openEdit}
      />
      <ScheduleFormSheet
        serverId={form.mode === "edit" ? form.serverId : undefined}
        visible={form.mode === "create" || form.mode === "edit"}
        onClose={closeForm}
        mode={form.mode === "edit" ? "edit" : "create"}
        schedule={form.mode === "edit" ? form.schedule : undefined}
      />
    </View>
  );
}

function SchedulesScreenBody({
  runnableRows,
  endedRows,
  loadState,
  showLoadError,
  showHostFilter,
  hosts,
  selectedHost,
  onSelectHost,
  onCreate,
  onEdit,
}: {
  runnableRows: ScheduleRowView[];
  endedRows: ScheduleRowView[];
  loadState: AggregateLoadState<AggregatedSchedule>;
  showLoadError: boolean;
  showHostFilter: boolean;
  hosts: ReturnType<typeof useHosts>;
  selectedHost: string;
  onSelectHost: (serverId: string) => void;
  onCreate: () => void;
  onEdit: (schedule: AggregatedSchedule) => void;
}): ReactElement {
  const { t } = useTranslation();
  const [endedVisibleCount, setEndedVisibleCount] = useState(ENDED_PAGE_SIZE);
  const bodyState = resolveSchedulesScreenBodyState({ loadState, showLoadError });

  if (bodyState.kind === "loading") {
    return (
      <View style={styles.centered}>
        <LoadingSpinner size="large" color={styles.spinner.color} />
      </View>
    );
  }

  return (
    <View style={styles.body}>
      <View style={styles.filterRow}>
        <View style={styles.filterRowControls}>
          {showHostFilter ? (
            <HostFilter
              hosts={hosts}
              selectedHost={selectedHost}
              onSelectHost={onSelectHost}
              triggerTestID="schedules-host-filter-trigger"
            />
          ) : null}
        </View>
        <Button
          variant="outline"
          leftIcon={Plus}
          onPress={onCreate}
          size="sm"
          testID="schedules-new"
        >
          {t("schedules.new")}
        </Button>
      </View>
      <ScrollView
        style={styles.scroll}
        contentContainerStyle={styles.scrollContent}
        showsVerticalScrollIndicator={false}
        keyboardShouldPersistTaps="handled"
        testID="schedules-list"
      >
        <SettingsSection
          title={t("schedules.activeCount", { count: runnableRows.length })}
          testID="schedules-active-section"
        >
          {runnableRows.length ? (
            <SchedulesTable rows={runnableRows} onEditSchedule={onEdit} />
          ) : (
            <SettingsCard>
              <SettingsRow
                label={t("schedules.empty")}
                hint={t("schedules.emptyDescription")}
                testID="schedules-empty"
              >
                <Button
                  variant="outline"
                  leftIcon={Plus}
                  size="sm"
                  onPress={onCreate}
                  testID="schedules-empty-new"
                >
                  {t("schedules.new")}
                </Button>
              </SettingsRow>
            </SettingsCard>
          )}
        </SettingsSection>
        <SettingsSection
          title={t("schedules.endedCount", { count: endedRows.length })}
          testID="schedules-ended-section"
          flush
        >
          {endedRows.length ? (
            <SchedulesTable rows={endedRows.slice(0, endedVisibleCount)} onEditSchedule={onEdit} />
          ) : (
            <SettingsCard>
              <SettingsRow label={t("schedules.endedEmpty")} />
            </SettingsCard>
          )}
          {endedRows.length > endedVisibleCount ? (
            <View style={styles.showMore}>
              <Button
                variant="ghost"
                size="sm"
                onPress={() => setEndedVisibleCount((count) => count + ENDED_PAGE_SIZE)}
                testID="schedules-ended-show-more"
              >
                {t("schedules.showMore")}
              </Button>
            </View>
          ) : null}
        </SettingsSection>
      </ScrollView>
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  container: {
    flex: 1,
    backgroundColor: theme.colors.surface0,
  },
  body: {
    flex: 1,
    minHeight: 0,
    padding: theme.spacing[4],
    paddingTop: theme.spacing[6],
    width: "100%",
    maxWidth: 720,
    alignSelf: "center",
  },
  centered: {
    flex: 1,
    justifyContent: "center",
    alignItems: "center",
    gap: theme.spacing[6],
    padding: theme.spacing[6],
  },
  filterRow: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: theme.spacing[3],
  },
  filterRowControls: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[3],
    flexShrink: 1,
    flexWrap: "wrap",
  },
  scroll: {
    flex: 1,
    minHeight: 0,
  },
  scrollContent: {
    flexGrow: 1,
    gap: theme.spacing[3],
    paddingTop: theme.spacing[4],
    paddingBottom: theme.spacing[6],
  },
  showMore: {
    alignItems: "center",
  },
  // Static color holder read by the spinner; keeps the muted token without
  // useUnistyles (banned in new code).
  spinner: {
    color: theme.colors.foregroundMuted,
  },
}));

import { useMemo, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { AdaptiveModalSheet } from "@/components/adaptive-modal-sheet";
import { ALL_HOSTS_OPTION_ID } from "@/components/hosts/host-picker";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { builtinSidebarNavLabelKey } from "@/sidebar-nav/model";
import { useHostUsageWithControls } from "./controls";
import { useUsageCopy } from "./copy";
import { useUsagePreferences } from "./display";
import { useUsageHostSelection } from "./hosts";
import { historyHosts, UsageHistoryPanel } from "./history";
import type { HistoryRange } from "./history-model";
import { useUsageHistoryHosts, type UsageHistoryHost } from "./history-queries";
import type { UsageHost } from "./model";
import { UsageBody, UsageMessage } from "./usage-section";

interface UsageModalProps {
  visible: boolean;
  onClose: () => void;
  onDismiss: () => void;
}

type UsageTab = "plan" | "history";

/** What the History tab shows; kept here so a tab switch or plan host change keeps it. */
interface HistoryChoice {
  hosts: UsageHistoryHost[];
  hostId: string;
  onSelectHost: (hostId: string) => void;
  range: HistoryRange;
  onSelectRange: (range: HistoryRange) => void;
}

/**
 * Usage over the current screen: a dialog on wide layouts, a bottom sheet on compact ones. It
 * shows the usage host's reports with pins, and the host filter, Force refresh, and Settings in its
 * title row. While a connected host keeps a usage ledger, a History tab sits beside the reports.
 */
export function UsageModal(props: UsageModalProps) {
  const usageCopy = useUsageCopy();
  const { serverId, connectedHosts, select } = useUsageHostSelection();
  const [tab, setTab] = useState<UsageTab>("plan");
  const [historyHostId, setHistoryHostId] = useState(ALL_HOSTS_OPTION_ID);
  const [historyRange, setHistoryRange] = useState<HistoryRange>("7d");
  const allHistoryHosts = useUsageHistoryHosts();
  const hosts = useMemo(() => historyHosts(allHistoryHosts), [allHistoryHosts]);
  const history = useMemo(
    () =>
      hosts.length > 0
        ? {
            hosts,
            hostId: historyHostId,
            onSelectHost: setHistoryHostId,
            range: historyRange,
            onSelectRange: setHistoryRange,
          }
        : null,
    [historyHostId, historyRange, hosts],
  );
  if (!serverId) {
    return (
      <UsageModalFrame {...props}>
        <UsageMessage text={usageCopy.noHosts} />
      </UsageModalFrame>
    );
  }
  // Keyed by host so a refresh in flight on one host never shows on another's cards.
  return (
    <HostUsageModal
      key={serverId}
      {...props}
      serverId={serverId}
      hosts={connectedHosts}
      onSelectHost={select}
      tab={history ? tab : "plan"}
      onSelectTab={setTab}
      history={history}
    />
  );
}

function HostUsageModal({
  serverId,
  hosts,
  onSelectHost,
  tab,
  onSelectTab,
  history,
  ...props
}: UsageModalProps & {
  serverId: string;
  hosts: UsageHost[];
  onSelectHost: (serverId: string) => void;
  tab: UsageTab;
  onSelectTab: (tab: UsageTab) => void;
  /** Null while no connected host keeps a usage ledger; the tabs are hidden then. */
  history: HistoryChoice | null;
}) {
  const usageCopy = useUsageCopy();
  const { display } = useUsagePreferences();
  const hostSelection = useMemo(
    () => ({ hosts, serverId, onSelect: onSelectHost }),
    [hosts, onSelectHost, serverId],
  );
  const { view, refresh, controls } = useHostUsageWithControls(hostSelection);
  const tabOptions = useMemo(
    () => [
      { value: "plan" as const, label: usageCopy.planUsage, testID: "usage-tab-plan" },
      { value: "history" as const, label: usageCopy.history.tab, testID: "usage-tab-history" },
    ],
    [usageCopy],
  );
  return (
    <UsageModalFrame {...props} actions={tab === "plan" ? controls : undefined}>
      {history ? (
        <SegmentedControl
          size="sm"
          options={tabOptions}
          value={tab}
          onValueChange={onSelectTab}
          style={styles.tabs}
          testID="usage-tabs"
        />
      ) : null}
      {tab === "history" && history ? (
        <UsageHistoryPanel {...history} />
      ) : (
        <View testID={`usage-host-${serverId}`}>
          <UsageBody serverId={serverId} view={view} display={display} onRefresh={refresh} />
        </View>
      )}
    </UsageModalFrame>
  );
}

function UsageModalFrame({
  visible,
  onClose,
  onDismiss,
  actions,
  children,
}: UsageModalProps & { actions?: ReactNode; children: ReactNode }) {
  const { t } = useTranslation();
  const title = t(builtinSidebarNavLabelKey("usage"));
  const header = useMemo(() => ({ title, actions }), [actions, title]);
  return (
    <AdaptiveModalSheet
      header={header}
      visible={visible}
      onClose={onClose}
      onDismiss={onDismiss}
      desktopMaxWidth={640}
      contextBridge={null}
      testID="usage-modal"
    >
      <View testID="usage-modal-body">{children}</View>
    </AdaptiveModalSheet>
  );
}

const styles = StyleSheet.create((theme) => ({
  tabs: {
    alignSelf: "flex-start",
    marginBottom: theme.spacing[3],
  },
}));

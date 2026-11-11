import { useMemo } from "react";
import { View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { RotateCw } from "lucide-react-native";
import { HostFilter } from "@/components/hosts/host-filter";
import { ALL_HOSTS_OPTION_ID } from "@/components/hosts/host-picker";
import { Button } from "@/components/ui/button";
import { SettingsSection } from "@/components/settings/headings/settings-section";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { useIsCompactFormFactor } from "@/constants/layout";
import { useHosts } from "@/runtime/host-runtime";
import { useUsageCopy } from "./copy";
import { HISTORY_RANGES, type HistoryRange } from "./history-model";
import { useHostHistoryTotals, useUsageHistory, type UsageHistoryHost } from "./history-queries";
import { HostHistoryTotalsBody, UsageHistoryBody } from "./history-tables";

/** The hosts History can show: connected, and offering the ledger. */
export function historyHosts(hosts: readonly UsageHistoryHost[]): UsageHistoryHost[] {
  return hosts.filter((host) => host.isConnected && host.supportsLedger === true);
}

/**
 * The Usage modal's History tab: the chosen range's calls and tokens on one host or all of them,
 * with the range, host filter, and Refresh above.
 */
export function UsageHistoryPanel({
  hosts,
  hostId,
  onSelectHost,
  range,
  onSelectRange,
}: {
  /** Hosts to pick from, as `historyHosts` gives them. */
  hosts: readonly UsageHistoryHost[];
  /** A host's server ID, or `ALL_HOSTS_OPTION_ID`. */
  hostId: string;
  onSelectHost: (hostId: string) => void;
  range: HistoryRange;
  onSelectRange: (range: HistoryRange) => void;
}) {
  const usageCopy = useUsageCopy();
  const compact = useIsCompactFormFactor();
  const hostProfiles = useHosts();
  // A picked host that went offline or away falls back to all hosts, where it is listed.
  const selected = hosts.some((host) => host.serverId === hostId) ? hostId : ALL_HOSTS_OPTION_ID;
  const { history, refresh } = useUsageHistory(
    selected === ALL_HOSTS_OPTION_ID ? null : selected,
    range,
  );
  const filterHosts = hostProfiles.filter((profile) =>
    hosts.some((host) => host.serverId === profile.serverId),
  );
  const rangeOptions = useMemo(
    () =>
      HISTORY_RANGES.map((value) => ({
        value,
        label: usageCopy.history.range(value),
        testID: `usage-history-range-${value}`,
      })),
    [usageCopy],
  );
  return (
    <View style={styles.panel} testID="usage-history">
      <View style={styles.toolbar}>
        <SegmentedControl
          size="sm"
          options={rangeOptions}
          value={range}
          onValueChange={onSelectRange}
        />
        <View style={styles.toolbarEnd}>
          {filterHosts.length > 1 ? (
            <HostFilter
              hosts={filterHosts}
              selectedHost={selected}
              onSelectHost={onSelectHost}
              triggerTestID="usage-history-host-filter-trigger"
            />
          ) : null}
          <Button
            variant="outline"
            size={compact ? "sm" : "xs"}
            leftIcon={RotateCw}
            loading={history.isRefreshing}
            onPress={refresh}
            testID="usage-history-refresh"
          >
            {usageCopy.refresh}
          </Button>
        </View>
      </View>
      <UsageHistoryBody history={history} onRetry={refresh} />
    </View>
  );
}

/** A host's calls, requests and tokens over the last 30 days, for its settings page. */
export function HostHistoryTotalsSection({ serverId }: { serverId: string }) {
  const usageCopy = useUsageCopy();
  const { totals, refresh } = useHostHistoryTotals(serverId);
  const hostLabel = useHosts().find((host) => host.serverId === serverId)?.label ?? serverId;
  if (totals.kind === "hidden") return null;
  return (
    <SettingsSection title={usageCopy.history.hostTotals} testID="usage-history-totals">
      <HostHistoryTotalsBody totals={totals} hostLabel={hostLabel} onRetry={refresh} />
    </SettingsSection>
  );
}

const styles = StyleSheet.create((theme) => ({
  panel: {
    gap: theme.spacing[3],
  },
  toolbar: {
    flexDirection: "row",
    flexWrap: "wrap",
    alignItems: "center",
    justifyContent: "space-between",
    gap: theme.spacing[2],
  },
  toolbarEnd: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
  },
}));

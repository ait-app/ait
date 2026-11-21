import { useMemo, useState } from "react";
import { View } from "react-native";
import { StyleSheet, withUnistyles } from "react-native-unistyles";
import { RotateCw } from "lucide-react-native";
import { SettingsSection } from "@/components/settings/headings/settings-section";
import { extraMutedIconColorMapping } from "@/components/ui/icon-button-chrome";
import { LoadingSpinner } from "@/components/ui/loading-spinner";
import { ToolbarButton, paneContentToolbarIconSize } from "@/components/ui/pane-content-toolbar";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { useIsCompactFormFactor } from "@/constants/layout";
import { useUsageCopy } from "./copy";
import { HISTORY_RANGES, type HistoryRange } from "./history-model";
import { useUsageHistory, useUsageHistoryHosts } from "./history-queries";
import { UsageHistoryBody } from "./history-tables";

const ThemedRotateCw = withUnistyles(RotateCw);
const ThemedLoadingSpinner = withUnistyles(LoadingSpinner);

/**
 * A host's usage history over the chosen range: totals, tokens per day, and the calls by provider,
 * model and day. Hidden while the host is offline or keeps no ledger.
 */
export function UsageHistorySection({ serverId }: { serverId: string }) {
  const usageCopy = useUsageCopy();
  const compact = useIsCompactFormFactor();
  const [range, setRange] = useState<HistoryRange>("7d");
  const host = useUsageHistoryHosts().find((candidate) => candidate.serverId === serverId);
  const { history, refresh } = useUsageHistory(serverId, range);
  const rangeOptions = useMemo(
    () =>
      HISTORY_RANGES.map((value) => ({
        value,
        label: usageCopy.history.range(value),
        testID: `usage-history-range-${value}`,
      })),
    [usageCopy],
  );
  if (!host?.isConnected || host.supportsLedger !== true) return null;
  const iconSize = paneContentToolbarIconSize(compact);
  const trailing = (
    <View style={styles.toolbar}>
      <SegmentedControl size="sm" options={rangeOptions} value={range} onValueChange={setRange} />
      <ToolbarButton
        label={usageCopy.refresh}
        compact={compact}
        disabled={history.isRefreshing}
        onPress={refresh}
        testID="usage-history-refresh"
      >
        {history.isRefreshing ? (
          <ThemedLoadingSpinner size={iconSize} uniProps={extraMutedIconColorMapping} />
        ) : (
          <ThemedRotateCw size={iconSize} uniProps={extraMutedIconColorMapping} />
        )}
      </ToolbarButton>
    </View>
  );
  return (
    <SettingsSection title={usageCopy.history.title} trailing={trailing} testID="usage-history">
      <UsageHistoryBody history={history} range={range} onRetry={refresh} />
    </SettingsSection>
  );
}

const styles = StyleSheet.create((theme) => ({
  toolbar: {
    flexShrink: 1,
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[1],
  },
}));

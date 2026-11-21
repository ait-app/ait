import { useMemo } from "react";
import { Text, View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { SettingsRow } from "@/components/settings";
import { SettingsSection } from "@/components/settings/headings/settings-section";
import { Button } from "@/components/ui/button";
import { SegmentedControl, type SegmentedControlOption } from "@/components/ui/segmented-control";
import { resolveHostGroupServerId } from "@/hosts/host-groups";
import { useHosts } from "@/runtime/host-runtime";
import { settingsStyles } from "@/styles/settings";
import { useUsageCopy } from "./copy";
import { useUsagePreferences } from "./display";
import { UsageHistorySection } from "./history";
import { useSelectUsageHost, useUsageHostId } from "./hosts";
import type { UsageDisplayAs } from "./preferences";
import { useHostUsage, useUsageHosts } from "./queries";
import { UsageRefreshButton } from "./refresh-button";
import { UsageBody } from "./usage-section";

/**
 * One host's Usage page in Settings: its plan usage, the history its ledger keeps, and how usage
 * reads on this device. Every host has its own page; nothing here sums hosts.
 */
export function UsageSettingsPage({ serverId }: { serverId: string }) {
  // Usage hosts are one per daemon; a shared host reached through several profiles reads as one.
  const groupServerId = resolveHostGroupServerId(useHosts(), serverId);
  return (
    <View testID={`usage-host-${groupServerId}`}>
      <PlanUsageSection serverId={serverId} />
      <UsageHistorySection serverId={groupServerId} />
      <UsageDisplaySection serverId={groupServerId} />
    </View>
  );
}

/** The host's reports, one card each, with Force refresh in the title row. */
function PlanUsageSection({ serverId }: { serverId: string }) {
  const usageCopy = useUsageCopy();
  const { view, refresh } = useHostUsage(serverId);
  const { display } = useUsagePreferences(serverId);
  const busy = view.kind === "loading" || (view.kind === "ready" && view.isRefreshing);
  const trailing = useMemo(
    () =>
      view.kind === "unavailable" ? null : <UsageRefreshButton busy={busy} onRefresh={refresh} />,
    [busy, refresh, view.kind],
  );
  return (
    <SettingsSection
      title={usageCopy.planUsage}
      info={usageCopy.pinInfo}
      trailing={trailing}
      testID="usage-card"
    >
      <UsageBody serverId={serverId} view={view} display={display} onRefresh={refresh} />
    </SettingsSection>
  );
}

/**
 * Device-local reading preferences: which host the sidebar summary reads when several report usage,
 * and whether percents count what is used or what is left.
 */
function UsageDisplaySection({ serverId }: { serverId: string }) {
  const usageCopy = useUsageCopy();
  const { display } = useUsagePreferences(serverId);
  const sidebarHostId = useUsageHostId();
  const selectHost = useSelectUsageHost();
  const reportingHosts = useUsageHosts().filter((host) => host.isConnected && host.supportsUsage);
  const choosesSidebarHost =
    reportingHosts.length > 1 && reportingHosts.some((host) => host.serverId === serverId);
  const displayAsOptions: SegmentedControlOption<UsageDisplayAs>[] = [
    { value: "used", label: usageCopy.displayUsed, testID: "usage-display-used" },
    { value: "remaining", label: usageCopy.displayRemaining, testID: "usage-display-remaining" },
  ];
  return (
    <SettingsSection title={usageCopy.displayTitle} testID="usage-display">
      <View style={settingsStyles.card}>
        {choosesSidebarHost ? (
          <SettingsRow
            label={usageCopy.sidebarHost}
            hint={usageCopy.sidebarHostHint}
            testID="usage-sidebar-host"
          >
            {sidebarHostId === serverId ? (
              <Text style={styles.current}>{usageCopy.sidebarHostCurrent}</Text>
            ) : (
              <Button
                variant="outline"
                size="sm"
                onPress={() => selectHost(serverId)}
                testID="usage-sidebar-host-use"
              >
                {usageCopy.sidebarHostUse}
              </Button>
            )}
          </SettingsRow>
        ) : null}
        <View style={choosesSidebarHost ? settingsStyles.rowBorder : null}>
          <SettingsRow label={usageCopy.displayAs}>
            <SegmentedControl
              options={displayAsOptions}
              value={display.displayAs}
              onValueChange={display.setDisplayAs}
              size="sm"
              testID="usage-display-as"
            />
          </SettingsRow>
        </View>
      </View>
    </SettingsSection>
  );
}

const styles = StyleSheet.create((theme) => ({
  current: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
}));

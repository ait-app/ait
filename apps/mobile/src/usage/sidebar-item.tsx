import { Fragment, useCallback, useMemo, useState } from "react";
import { router } from "expo-router";
import { useTranslation } from "react-i18next";
import {
  Pressable,
  Text,
  View,
  type LayoutChangeEvent,
  type PressableStateCallbackType,
} from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { useUsagePreferences } from "./display";
import { buildSettingsHostSectionRoute, buildSettingsRoute } from "@/utils/host-routes";
import { useUsageHostId, useUsageSettingsHostId } from "./hosts";
import { useUsageHostReports } from "./queries";
import { UsageSourceIcon } from "./source-icon";
import { UsageMeter } from "./meter";
import {
  choosePinnedUsageLayout,
  METER_GAP,
  MIN_METER_WIDTH,
  resolvePinnedUsage,
  type PinnedUsageLayout,
  type PinnedUsageSource,
} from "./pinned";

/** Each summary window with data on the usage host, under its source; empty while none has. */
function useUsageSummary(): readonly PinnedUsageSource[] {
  const { t } = useTranslation();
  const { preferences } = useUsagePreferences();
  const reports = useUsageHostReports(useUsageHostId());
  return useMemo(() => resolvePinnedUsage(reports, preferences, t), [preferences, reports, t]);
}

/**
 * The sidebar footer's usage summary: each pinned window's source icon and percent. Nothing while
 * no pinned window has data, so unpinning every window hides it. Pressing it opens the host's
 * Usage page in Settings.
 */
export function UsageSidebarItem({ onBeforeNavigate }: { onBeforeNavigate?: () => void }) {
  const { t } = useTranslation();
  const label = t("sidebar.sections.usage");
  const openUsage = useOpenUsageSettings(onBeforeNavigate);
  const sources = useUsageSummary();
  if (sources.length === 0) return null;
  return <PinnedUsageTrigger label={label} sources={sources} onPress={openUsage} />;
}

/**
 * Opens Usage, which lives on each host's settings page: the page of the host the sidebar summary
 * reads, or Settings itself while there is no host. `onBeforeNavigate` closes a compact sidebar.
 */
function useOpenUsageSettings(onBeforeNavigate?: () => void): () => void {
  const serverId = useUsageSettingsHostId();
  return useCallback(() => {
    onBeforeNavigate?.();
    router.push(serverId ? buildSettingsHostSectionRoute(serverId, "usage") : buildSettingsRoute());
  }, [onBeforeNavigate, serverId]);
}

function pinnedUsageLabel(label: string, sources: readonly PinnedUsageSource[]): string {
  const windows = sources.flatMap((source) => source.windows);
  return `${label}: ${windows.map((window) => window.label).join(", ")}`;
}

function triggerStyle({ hovered }: PressableStateCallbackType & { hovered?: boolean }) {
  return hovered ? [styles.trigger, styles.triggerHovered] : styles.trigger;
}

function layoutWidth(event: LayoutChangeEvent): number {
  return event.nativeEvent.layout.width;
}

/**
 * One line: each account's icon, then per window a meter and "31% 5h". Off-screen copies measure
 * the line with labels and with percents alone; the richest layout that fits is shown, and what
 * it leaves out takes no space at all.
 */
function PinnedUsageTrigger({
  label,
  sources,
  onPress,
}: {
  label: string;
  sources: readonly PinnedUsageSource[];
  onPress: () => void;
}) {
  const [available, setAvailable] = useState<number | null>(null);
  const [labelsWidth, setLabelsWidth] = useState<number | null>(null);
  const [percentsWidth, setPercentsWidth] = useState<number | null>(null);
  const handleAvailableLayout = useCallback((event: LayoutChangeEvent) => {
    setAvailable(layoutWidth(event));
  }, []);
  const handleLabelsLayout = useCallback((event: LayoutChangeEvent) => {
    setLabelsWidth(layoutWidth(event));
  }, []);
  const handlePercentsLayout = useCallback((event: LayoutChangeEvent) => {
    setPercentsWidth(layoutWidth(event));
  }, []);
  const layout = choosePinnedUsageLayout({
    available,
    labelsWidth,
    percentsWidth,
    windowCount: sources.reduce((count, source) => count + source.windows.length, 0),
  });
  return (
    <Pressable
      onPress={onPress}
      accessibilityRole="button"
      accessibilityLabel={pinnedUsageLabel(label, sources)}
      style={triggerStyle}
      testID="sidebar-usage"
    >
      <View style={styles.line} onLayout={handleAvailableLayout}>
        <PinnedUsageLine sources={sources} layout={layout} />
      </View>
      <View
        style={styles.measure}
        pointerEvents="none"
        accessibilityElementsHidden
        importantForAccessibility="no-hide-descendants"
        aria-hidden
      >
        <View style={styles.measured} onLayout={handleLabelsLayout}>
          <PinnedUsageLine sources={sources} layout="labels" measuring />
        </View>
        <View style={styles.measured} onLayout={handlePercentsLayout}>
          <PinnedUsageLine sources={sources} layout="percents" measuring />
        </View>
      </View>
    </Pressable>
  );
}

/**
 * The line's items as direct children of one row, so every meter shares the leftover width
 * equally.
 */
function PinnedUsageLine({
  sources,
  layout,
  measuring = false,
}: {
  sources: readonly PinnedUsageSource[];
  layout: PinnedUsageLayout;
  measuring?: boolean;
}) {
  return sources.map((source, sourceIndex) => (
    <Fragment key={source.key}>
      <View
        style={sourceIndex === 0 ? null : styles.sourceGap}
        testID={measuring ? undefined : "sidebar-usage-source"}
      >
        <UsageSourceIcon sourceId={source.key.split(":")[0]} svg={source.icon} size={14} />
      </View>
      {source.windows.map((window, windowIndex) => {
        const leadStyle = windowIndex === 0 ? styles.iconGap : styles.windowGap;
        return (
          <Fragment key={window.key}>
            {layout === "meters" ? (
              <View style={[styles.meterSlot, leadStyle]}>
                <UsageMeter
                  percent={window.percent}
                  tone={window.tone}
                  quiet
                  style={styles.meter}
                />
              </View>
            ) : null}
            <Text
              style={[styles.percent, layout === "meters" ? null : leadStyle]}
              numberOfLines={1}
              testID={measuring ? undefined : "sidebar-usage-pinned-window"}
            >
              {window.percentText}
              {layout === "percents" || window.shortLabel === "" ? null : (
                <Text style={styles.windowLabel}>{` ${window.shortLabel}`}</Text>
              )}
            </Text>
          </Fragment>
        );
      })}
    </Fragment>
  ));
}

const styles = StyleSheet.create((theme) => ({
  trigger: {
    width: "100%",
    flexDirection: "row",
    alignItems: "center",
    overflow: "hidden",
    // Same row geometry and leading rail as Add project and the footer icons.
    minHeight: 28,
    paddingVertical: theme.spacing[1],
    paddingHorizontal: theme.spacing[2],
    borderRadius: theme.borderRadius.lg,
  },
  triggerHovered: {
    backgroundColor: theme.colors.surfaceSidebarHover,
  },
  line: {
    flex: 1,
    minWidth: 0,
    flexDirection: "row",
    alignItems: "center",
    overflow: "hidden",
  },
  // Wide enough that nothing inside wraps or shrinks; each child reports its natural width.
  measure: {
    position: "absolute",
    top: 0,
    left: 0,
    width: 10_000,
    opacity: 0,
  },
  measured: {
    alignSelf: "flex-start",
    flexDirection: "row",
    alignItems: "center",
  },
  sourceGap: {
    marginLeft: theme.spacing[3],
  },
  iconGap: {
    marginLeft: theme.spacing[1.5],
  },
  windowGap: {
    marginLeft: theme.spacing[2],
  },
  // Every slot takes an equal share of the leftover width, so the line fills the row.
  meterSlot: {
    flexGrow: 1,
    flexShrink: 1,
    flexBasis: 0,
    minWidth: MIN_METER_WIDTH,
    marginRight: METER_GAP,
  },
  meter: {
    height: 6,
    borderRadius: 3,
  },
  percent: {
    flexShrink: 0,
    color: theme.colors.foreground,
    fontSize: theme.fontSize.sm,
    fontVariant: ["tabular-nums"],
  },
  windowLabel: {
    color: theme.colors.foregroundMuted,
  },
}));

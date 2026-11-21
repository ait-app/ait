import { useCallback, useState } from "react";
import { View, type LayoutChangeEvent } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { settingsStyles } from "@/styles/settings";
import { UsageCard } from "./card";
import type { UsageDisplay } from "./display";
import type { UsageReportEntry } from "./types";

/** Below this width cards stack; at or above it, two sit side by side. */
const TWO_COLUMN_MIN_WIDTH = 560;
const CARD_GAP = 12;

/**
 * One card per report (source + account), two to a row where the page is wide enough. Each window
 * row pins the window to the sidebar summary.
 */
export function UsageList({
  serverId,
  reports,
  display,
}: {
  serverId: string;
  reports: UsageReportEntry[];
  display: UsageDisplay;
}) {
  const [width, setWidth] = useState(0);
  const handleLayout = useCallback((event: LayoutChangeEvent) => {
    setWidth(event.nativeEvent.layout.width);
  }, []);
  const columnWidth =
    reports.length > 1 && width >= TWO_COLUMN_MIN_WIDTH ? (width - CARD_GAP) / 2 : null;
  return (
    <View style={styles.list} onLayout={handleLayout} testID="usage-report-list">
      {reports.map((entry) => (
        <View
          key={entry.id}
          style={[settingsStyles.card, columnWidth === null ? styles.full : { width: columnWidth }]}
        >
          <UsageCard serverId={serverId} entry={entry} display={display} pinnable refreshable />
        </View>
      ))}
    </View>
  );
}

const styles = StyleSheet.create({
  list: {
    flexDirection: "row",
    flexWrap: "wrap",
    gap: CARD_GAP,
  },
  full: {
    width: "100%",
  },
});

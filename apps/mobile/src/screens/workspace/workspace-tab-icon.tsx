import { useMemo, type ReactElement } from "react";
import { View } from "react-native";
import { CircleAlert } from "lucide-react-native";
import { StyleSheet, withUnistyles } from "react-native-unistyles";
import type { SurfaceBackdrop } from "@/styles/surface-backdrop";
import { getStatusDotColor } from "@/utils/status-dot-color";
import { StatusRing } from "@/components/status-ring";
import { getStatusRingOffset } from "@/components/status-ring/geometry";
import {
  STATUS_INDICATOR_ALERT_SIZE,
  STATUS_INDICATOR_DOT_SIZE,
} from "@/utils/status-indicator-geometry";
import type { Theme } from "@/styles/theme";
import type { WorkspaceTabPresentation } from "@/screens/workspace/workspace-tab-presentation";

const DEFAULT_STATUS_DOT_OFFSET = -2;
const STATUS_ALERT_OFFSET = -3;

interface WorkspaceTabIconProps {
  presentation: WorkspaceTabPresentation;
  active?: boolean;
  size?: number;
  strokeWidth?: number;
  statusDotBorderColor?: string;
  /**
   * The surface this icon is sitting on, so the running ring can knock out of it. This icon is
   * shared by the desktop tab strip, the tab switcher trigger, the split drag chip and the
   * subagents track, which are on different surfaces and some of which change surface on hover —
   * it cannot work the colour out for itself.
   */
  backdrop: SurfaceBackdrop;
}

const ThemedCircleAlert = withUnistyles(CircleAlert);
const needsInputAlertMapping = (theme: Theme) => ({
  color: theme.colors.surface0,
  fill: getStatusDotColor({ theme, bucket: "needs_input" }) ?? undefined,
});

export function WorkspaceTabIcon({
  presentation,
  active = false,
  size = 14,
  strokeWidth,
  statusDotBorderColor,
  backdrop,
}: WorkspaceTabIconProps): ReactElement {
  const iconColor = active ? styles.iconActive.color : styles.iconInactive.color;
  const bucket = presentation.statusBucket;
  const isRunning = bucket === "running";
  let statusDotColor: string | undefined;
  if (bucket === "failed") statusDotColor = styles.statusDotFailed.color;
  else if (bucket === "attention") statusDotColor = styles.statusDotAttention.color;
  const showNeedsInputAlert = bucket === "needs_input";
  const Icon = presentation.icon;
  const agentIconWrapperStyle = useMemo(
    () => [styles.agentIconWrapper, { width: size, height: size }],
    [size],
  );
  const statusDotStyle = useMemo(
    () => [
      styles.statusDot,
      {
        backgroundColor: statusDotColor,
        borderColor: statusDotBorderColor ?? styles.statusDotBorderDefault.borderColor,
      },
    ],
    [statusDotColor, statusDotBorderColor],
  );

  return (
    <View style={agentIconWrapperStyle}>
      <Icon size={size} color={iconColor} strokeWidth={strokeWidth} />
      {isRunning ? (
        <View
          style={styles.statusRing}
          accessibilityRole="progressbar"
          accessibilityLabel="Agent running"
        >
          <StatusRing backdrop={backdrop} />
        </View>
      ) : null}
      {statusDotColor ? <View style={statusDotStyle} /> : null}
      {showNeedsInputAlert ? (
        <View style={styles.statusAlertOverlay}>
          <ThemedCircleAlert size={STATUS_INDICATOR_ALERT_SIZE} uniProps={needsInputAlertMapping} />
        </View>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  agentIconWrapper: {
    position: "relative",
    alignItems: "center",
    justifyContent: "center",
    flexShrink: 0,
  },
  statusDot: {
    position: "absolute",
    right: DEFAULT_STATUS_DOT_OFFSET,
    bottom: DEFAULT_STATUS_DOT_OFFSET,
    width: STATUS_INDICATOR_DOT_SIZE,
    height: STATUS_INDICATOR_DOT_SIZE,
    borderRadius: theme.borderRadius.full,
    borderWidth: 1,
  },
  statusRing: {
    position: "absolute",
    right: getStatusRingOffset(DEFAULT_STATUS_DOT_OFFSET, STATUS_INDICATOR_DOT_SIZE),
    bottom: getStatusRingOffset(DEFAULT_STATUS_DOT_OFFSET, STATUS_INDICATOR_DOT_SIZE),
  },
  statusDotBorderDefault: {
    borderColor: theme.colors.surface0,
  },
  statusAlertOverlay: {
    position: "absolute",
    right: STATUS_ALERT_OFFSET,
    bottom: STATUS_ALERT_OFFSET,
  },
  statusDotFailed: {
    color: getStatusDotColor({ theme, bucket: "failed" }) ?? undefined,
  },
  statusDotAttention: {
    color: getStatusDotColor({ theme, bucket: "attention" }) ?? undefined,
  },
  iconActive: {
    color: theme.colors.foreground,
  },
  iconInactive: {
    color: theme.colors.foregroundMuted,
  },
}));

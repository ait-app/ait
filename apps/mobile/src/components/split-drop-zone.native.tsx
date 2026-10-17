import { useMemo } from "react";
import { View } from "react-native";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import type { SplitDropZoneHover, SplitDropZonePosition } from "./split-drop-zone-geometry";

export { resolveSplitDropPosition } from "./split-drop-zone-geometry";
export type { SplitDropZoneHover, SplitDropZonePosition } from "./split-drop-zone-geometry";

export interface SplitDropZoneProps {
  paneId: string;
  active: boolean;
  preview: SplitDropZoneHover | null;
}

export function SplitDropZone({ paneId, active, preview }: SplitDropZoneProps) {
  const { theme } = useUnistyles();
  const previewStyle = useMemo(() => {
    if (!preview || preview.paneId !== paneId) {
      return null;
    }
    return [
      styles.preview,
      getPreviewStyle(preview.position),
      {
        backgroundColor: theme.colors.accent,
        borderColor: theme.colors.accent,
      },
    ];
  }, [paneId, preview, theme.colors.accent]);

  if (!active) {
    return null;
  }

  return (
    <View style={styles.overlay} pointerEvents="none">
      {previewStyle ? <View style={previewStyle} pointerEvents="none" /> : null}
    </View>
  );
}

function getPreviewStyle(position: SplitDropZonePosition) {
  if (position === "left") return styles.left;
  if (position === "right") return styles.right;
  if (position === "top") return styles.top;
  if (position === "bottom") return styles.bottom;
  return styles.center;
}

const styles = StyleSheet.create((theme) => ({
  overlay: {
    ...StyleSheet.absoluteFillObject,
    zIndex: 40,
  },
  preview: {
    position: "absolute",
    borderRadius: theme.borderRadius.md,
    borderWidth: 2,
    opacity: 0.28,
  },
  left: { left: 0, top: 0, bottom: 0, width: "50%" },
  right: { right: 0, top: 0, bottom: 0, width: "50%" },
  top: { left: 0, top: 0, right: 0, height: "50%" },
  bottom: { left: 0, right: 0, bottom: 0, height: "50%" },
  center: {
    left: theme.spacing[2],
    top: theme.spacing[2],
    right: theme.spacing[2],
    bottom: theme.spacing[2],
  },
}));

import { useMemo, type Ref } from "react";
import { useDroppable } from "@dnd-kit/core";
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

export function buildSplitDropZoneId(paneId: string): string {
  return `split-pane-drop:${paneId}`;
}

export function SplitDropZone({ paneId, active, preview }: SplitDropZoneProps) {
  const { theme } = useUnistyles();
  const { setNodeRef } = useDroppable({
    id: buildSplitDropZoneId(paneId),
    disabled: !active,
    data: {
      kind: "split-pane-drop",
      paneId,
    },
  });

  const previewStyles = useMemo(() => {
    if (!preview || preview.paneId !== paneId) {
      return null;
    }
    return {
      overlay: [
        styles.previewOverlay,
        getPreviewOverlayStyle(preview.position),
        {
          backgroundColor: theme.colors.accent,
          opacity: 0.6,
        },
      ],
      frame: [
        styles.previewFrame,
        getPreviewFrameStyle(preview.position),
        {
          borderColor: theme.colors.accent,
        },
      ],
    };
  }, [paneId, preview, theme.colors.accent]);

  if (!active) {
    return null;
  }

  return (
    <View ref={setNodeRef as unknown as Ref<View>} style={styles.overlay} pointerEvents="none">
      {previewStyles ? (
        <>
          <View pointerEvents="none" style={previewStyles.overlay} />
          <View pointerEvents="none" style={previewStyles.frame} />
        </>
      ) : null}
    </View>
  );
}

function getPreviewOverlayStyle(position: SplitDropZonePosition) {
  if (position === "left") {
    return styles.previewLeft;
  }
  if (position === "right") {
    return styles.previewRight;
  }
  if (position === "top") {
    return styles.previewTop;
  }
  if (position === "bottom") {
    return styles.previewBottom;
  }
  return styles.previewCenterOverlay;
}

function getPreviewFrameStyle(position: SplitDropZonePosition) {
  if (position === "left") {
    return styles.previewLeft;
  }
  if (position === "right") {
    return styles.previewRight;
  }
  if (position === "top") {
    return styles.previewTop;
  }
  if (position === "bottom") {
    return styles.previewBottom;
  }
  return styles.previewCenterFrame;
}

const styles = StyleSheet.create((theme) => ({
  overlay: {
    ...StyleSheet.absoluteFillObject,
    zIndex: 40,
  },
  previewOverlay: {
    position: "absolute",
    borderRadius: theme.borderRadius.md,
  },
  previewFrame: {
    position: "absolute",
    borderRadius: theme.borderRadius.md,
    borderWidth: 2,
  },
  previewLeft: {
    left: 0,
    top: 0,
    bottom: 0,
    width: "50%",
  },
  previewRight: {
    right: 0,
    top: 0,
    bottom: 0,
    width: "50%",
  },
  previewTop: {
    left: 0,
    top: 0,
    right: 0,
    height: "50%",
  },
  previewBottom: {
    left: 0,
    right: 0,
    bottom: 0,
    height: "50%",
  },
  previewCenterOverlay: {
    left: theme.spacing[2],
    top: theme.spacing[2],
    right: theme.spacing[2],
    bottom: theme.spacing[2],
  },
  previewCenterFrame: {
    left: theme.spacing[2],
    top: theme.spacing[2],
    right: theme.spacing[2],
    bottom: theme.spacing[2],
  },
}));

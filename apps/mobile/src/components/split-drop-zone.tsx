import { useMemo, type Ref } from "react";
import { useDroppable } from "@dnd-kit/core";
import { View } from "react-native";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import type { SplitDropZoneHover, SplitDropZonePosition } from "./split-drop-zone-geometry";

interface SplitDropZoneProps {
  paneId: string;
  active: boolean;
  preview: SplitDropZoneHover | null;
}

export function SplitDropZone({ paneId, active, preview }: SplitDropZoneProps) {
  const { theme } = useUnistyles();
  const { setNodeRef } = useDroppable({
    id: `split-pane-drop:${paneId}`,
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
        getPreviewPositionStyle(preview.position),
        {
          backgroundColor: theme.colors.accent,
          opacity: 0.6,
        },
      ],
      frame: [
        styles.previewFrame,
        getPreviewPositionStyle(preview.position),
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

function getPreviewPositionStyle(position: SplitDropZonePosition) {
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
  return styles.previewCenter;
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
  previewCenter: {
    left: theme.spacing[2],
    top: theme.spacing[2],
    right: theme.spacing[2],
    bottom: theme.spacing[2],
  },
}));

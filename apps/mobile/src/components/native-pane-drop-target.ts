import {
  resolveSplitDropPosition,
  type SplitDropZoneHover,
} from "@/components/split-drop-zone-geometry";

export interface NativePaneFrame {
  paneId: string;
  left: number;
  top: number;
  width: number;
  height: number;
}

export function resolveNativePaneDropTarget(input: {
  x: number;
  y: number;
  panes: NativePaneFrame[];
}): SplitDropZoneHover | null {
  const pane = input.panes.find(
    (candidate) =>
      candidate.width > 0 &&
      candidate.height > 0 &&
      input.x >= candidate.left &&
      input.x <= candidate.left + candidate.width &&
      input.y >= candidate.top &&
      input.y <= candidate.top + candidate.height,
  );
  if (!pane) {
    return null;
  }

  return {
    paneId: pane.paneId,
    position: resolveSplitDropPosition({
      width: pane.width,
      height: pane.height,
      x: input.x - pane.left,
      y: input.y - pane.top,
    }),
  };
}

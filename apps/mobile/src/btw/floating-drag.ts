/**
 * A floating window is laid out from its bottom edge, so it can rise above a composer that grows,
 * while a drag speaks in top-down screen coordinates. Both representations and the travel limits
 * live here so the layout hook and the drag hook share one definition.
 */
export interface BtwFloatingFrame {
  left: number;
  bottom: number;
  width: number;
  height: number;
}

/** Where a window may travel, in screen coordinates. */
export interface BtwDragBounds {
  minLeft: number;
  maxLeft: number;
  minTop: number;
  maxTop: number;
}

/** A window position in screen coordinates. */
export interface BtwWindowPosition {
  left: number;
  top: number;
}

export function btwFramePosition(frame: BtwFloatingFrame, layoutHeight: number): BtwWindowPosition {
  return { left: frame.left, top: layoutHeight - frame.bottom - frame.height };
}

export function placeBtwFrame(
  frame: BtwFloatingFrame,
  position: BtwWindowPosition,
  layoutHeight: number,
): BtwFloatingFrame {
  return {
    ...frame,
    left: position.left,
    bottom: layoutHeight - position.top - frame.height,
  };
}

export function clampBtwPosition(
  position: BtwWindowPosition,
  bounds: BtwDragBounds,
): BtwWindowPosition {
  return {
    left: Math.min(Math.max(position.left, bounds.minLeft), bounds.maxLeft),
    top: Math.min(Math.max(position.top, bounds.minTop), bounds.maxTop),
  };
}

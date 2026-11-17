import { useCallback, useMemo, useRef, useState } from "react";
import {
  PanResponder,
  type PanResponderGestureState,
  type PointerEvent as RNPointerEvent,
  type ViewProps,
  type ViewStyle,
} from "react-native";
import { isWeb } from "@/constants/platform";
import {
  btwFramePosition,
  clampBtwPosition,
  placeBtwFrame,
  type BtwDragBounds,
  type BtwFloatingFrame,
  type BtwWindowPosition,
} from "@/btw/floating-drag";

export interface BtwDragInput {
  /** Where the composer alone puts the window, before any drag. */
  frame: BtwFloatingFrame | null;
  /** Where the window may be placed: the viewport inset by the edge gap and the safe area. */
  dragBounds: BtwDragBounds | null;
  layoutHeight: number;
}

/** Props that turn an element into the handle that moves the floating window. */
export interface BtwDragHandleProps {
  onPointerDown?: (event: RNPointerEvent) => void;
  onStartShouldSetResponder?: ViewProps["onStartShouldSetResponder"];
  onMoveShouldSetResponder?: ViewProps["onMoveShouldSetResponder"];
  onMoveShouldSetResponderCapture?: ViewProps["onMoveShouldSetResponderCapture"];
  onResponderMove?: ViewProps["onResponderMove"];
  onResponderRelease?: ViewProps["onResponderRelease"];
  onResponderTerminate?: ViewProps["onResponderTerminate"];
  onResponderTerminationRequest?: ViewProps["onResponderTerminationRequest"];
  style?: ViewProps["style"];
}

export interface BtwDragProps {
  /** Props for the window header: the surface the user grabs to move the window. */
  headerProps: BtwDragHandleProps;
  /** The frame to draw, with the user's own placement applied and kept inside `dragBounds`. */
  frame: BtwFloatingFrame | null;
  dragging: boolean;
}

interface DragOrigin {
  pointerX: number;
  pointerY: number;
  position: BtwWindowPosition;
}

/** Movement below this is a click on a header control, not a drag. */
const DRAG_THRESHOLD = 3;

/**
 * A window the user places by dragging its header.
 *
 * The chosen corner is stored rather than a delta, so a composer that grows, a keyboard that opens
 * or a window that resizes re-clamps the same spot instead of pushing the window back to where the
 * composer happened to be at drag time. The header only claims the gesture once it moves, so its
 * controls keep receiving their own taps.
 */
export function useBtwDrag(input: BtwDragInput): BtwDragProps {
  const [position, setPosition] = useState<BtwWindowPosition | null>(null);
  const [dragging, setDragging] = useState(false);
  const origin = useRef<DragOrigin | null>(null);
  const draggingRef = useRef(false);
  // The gesture reads the latest input, not the values captured when the header first mounted.
  const current = useRef(input);
  current.current = input;

  const { frame, dragBounds, layoutHeight } = input;
  // Mirrors `position` for the gesture callbacks, which outlive the render that installed them.
  const positionRef = useRef<BtwWindowPosition | null>(null);
  const placed =
    frame && position && dragBounds
      ? placeBtwFrame(frame, clampBtwPosition(position, dragBounds), layoutHeight)
      : frame;

  /** Where the window is drawn now: a drag starts here, not at the composer's own frame. */
  const grabStart = useCallback((): BtwWindowPosition | null => {
    const latest = current.current;
    if (!latest.frame || !latest.dragBounds) return null;
    const base = positionRef.current ?? btwFramePosition(latest.frame, latest.layoutHeight);
    return clampBtwPosition(base, latest.dragBounds);
  }, []);

  const followDrag = useCallback((state: DragOrigin, pointerX: number, pointerY: number) => {
    const { dragBounds: limits } = current.current;
    if (!limits) return;
    const dx = pointerX - state.pointerX;
    const dy = pointerY - state.pointerY;
    if (!draggingRef.current && Math.abs(dx) < DRAG_THRESHOLD && Math.abs(dy) < DRAG_THRESHOLD) {
      return;
    }
    if (!draggingRef.current) {
      draggingRef.current = true;
      setDragging(true);
    }
    const next = clampBtwPosition(
      { left: state.position.left + dx, top: state.position.top + dy },
      limits,
    );
    positionRef.current = next;
    setPosition(next);
  }, []);

  const reset = useCallback(() => {
    origin.current = null;
    draggingRef.current = false;
    setDragging(false);
  }, []);

  const onPointerDown = useCallback(
    (event: RNPointerEvent) => {
      const start = grabStart();
      if (!start) return;
      // RN wraps DOM events in `nativeEvent`; the raw DOM event arrives unwrapped.
      const native = (event.nativeEvent ?? event) as unknown as {
        clientX?: number;
        clientY?: number;
        pageX?: number;
        pageY?: number;
      };
      const x = Number(native.clientX ?? native.pageX);
      const y = Number(native.clientY ?? native.pageY);
      if (!Number.isFinite(x) || !Number.isFinite(y)) return;
      const state: DragOrigin = { pointerX: x, pointerY: y, position: start };
      origin.current = state;
      const move = (candidate: PointerEvent) => {
        candidate.preventDefault();
        followDrag(state, candidate.clientX, candidate.clientY);
      };
      const up = () => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", up);
        window.removeEventListener("pointercancel", up);
        reset();
      };
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", up);
      window.addEventListener("pointercancel", up);
    },
    [followDrag, grabStart, reset],
  );

  const panResponder = useMemo(() => {
    const isDrag = (gesture: PanResponderGestureState) =>
      Math.abs(gesture.dx) > DRAG_THRESHOLD || Math.abs(gesture.dy) > DRAG_THRESHOLD;
    return PanResponder.create({
      // Claim the touch only once it becomes a drag, so header controls keep their taps. The
      // grab point is read on grant, which only happens for a gesture that turned into a drag.
      onStartShouldSetPanResponder: () => false,
      onMoveShouldSetPanResponder: (_event, gesture) => isDrag(gesture),
      onMoveShouldSetPanResponderCapture: (_event, gesture) => isDrag(gesture),
      onPanResponderGrant: () => {
        const start = grabStart();
        if (start) origin.current = { pointerX: 0, pointerY: 0, position: start };
      },
      onPanResponderTerminationRequest: () => false,
      onPanResponderMove: (_event, gesture) => {
        const state = origin.current;
        if (state) followDrag(state, gesture.dx, gesture.dy);
      },
      onPanResponderRelease: reset,
      onPanResponderTerminate: reset,
    });
  }, [followDrag, grabStart, reset]);

  const headerProps = useMemo(
    () =>
      isWeb
        ? {
            onPointerDown,
            // RN's style types cover native props only; these three are web-only.
            style: {
              cursor: dragging ? "grabbing" : "grab",
              userSelect: "none",
              touchAction: "none",
            } as unknown as ViewStyle,
          }
        : { ...panResponder.panHandlers },
    [dragging, onPointerDown, panResponder],
  );

  return { headerProps, frame: placed, dragging };
}

import { useLayoutEffect, useState, type RefObject } from "react";
import { useWindowDimensions, type View } from "react-native";
import { useSafeAreaInsets, type EdgeInsets } from "react-native-safe-area-context";
import { isWeb } from "@/constants/platform";
import { useSettledKeyboardShift } from "@/keyboard/shift";
import { measureElement, type Rect, type Size } from "@/components/ui/menu/menu-anchor";
import { SPACING } from "@/styles/theme";
import type { BtwDragBounds, BtwFloatingFrame } from "@/btw/floating-drag";

export type { BtwFloatingFrame };

interface Viewport extends Rect {
  layoutHeight: number;
}

/** Gap kept from every edge of the window, and from the composer the window opens above. */
const EDGE = SPACING[2];
const COMPOSER_GAP = SPACING[3];
const MAX_WIDTH = 384;
const MAX_HEIGHT = 420;
const MIN_TYPING_HEIGHT = 128;

/**
 * The window opens above its composer, hugging the right edge of the window. The composer is
 * centred in a reading column, so its own right edge sits far inside a wide layout; the window
 * edge is where a floating window is looked for. A window too narrow for the panel shrinks the
 * panel instead of pushing it outside.
 */
export function resolveBtwFloatingFrame(
  anchor: Rect,
  viewport: Viewport,
  safeTop: number,
): BtwFloatingFrame {
  const width = Math.max(0, Math.min(MAX_WIDTH, viewport.width - EDGE * 2));
  const top = viewport.y + safeTop + EDGE;
  const bottom = Math.min(anchor.y - COMPOSER_GAP, viewport.y + viewport.height - EDGE);
  return {
    left: Math.max(viewport.x + EDGE, viewport.x + viewport.width - EDGE - width),
    bottom: viewport.layoutHeight - bottom,
    width,
    height: Math.max(0, Math.min(MAX_HEIGHT, bottom - top)),
  };
}

/** The box a dragged window may travel in: the whole window, minus an edge gap. */
export function resolveBtwDragBounds(
  viewport: Viewport,
  safeTop: number,
  size: { width: number; height: number },
): BtwDragBounds {
  const minLeft = viewport.x + EDGE;
  const minTop = viewport.y + safeTop + EDGE;
  return {
    minLeft,
    minTop,
    maxLeft: Math.max(minLeft, viewport.x + viewport.width - EDGE - size.width),
    maxTop: Math.max(minTop, viewport.y + viewport.height - EDGE - size.height),
  };
}

export interface BtwFloatingLayout {
  /** Where the composer alone would put the window, before any drag. */
  frame: BtwFloatingFrame | null;
  dragBounds: BtwDragBounds | null;
  /** Native animation starts from this box and applies the live keyboard overlap. */
  restingDragBounds?: BtwDragBounds;
  layoutHeight: number;
}

export function resolveBtwNativeLayout(
  anchor: Rect,
  size: Size,
  insets: EdgeInsets,
  keyboardShift: number,
) {
  const restingViewport = {
    x: insets.left,
    y: 0,
    width: Math.max(0, size.width - insets.left - insets.right),
    height: Math.max(0, size.height - insets.bottom),
    layoutHeight: size.height,
  };
  const viewport = {
    ...restingViewport,
    height: Math.max(0, restingViewport.height - keyboardShift),
  };
  const docked = resolveBtwFloatingFrame(anchor, restingViewport, insets.top);
  const availableHeight = Math.max(0, viewport.height - insets.top - EDGE * 2);
  // A short gap above the main composer should still leave room to type. Let
  // the floating window overlap that composer if the keyboard leaves no other space.
  const height = Math.min(
    Math.max(docked.height, keyboardShift > 0 ? MIN_TYPING_HEIGHT : 0),
    availableHeight,
  );
  const frame = {
    ...docked,
    bottom: Math.min(docked.bottom, size.height - insets.top - EDGE - height),
    height,
  };
  return {
    frame,
    dragBounds: resolveBtwDragBounds(viewport, insets.top, frame),
    restingDragBounds: resolveBtwDragBounds(restingViewport, insets.top, frame),
    layoutHeight: size.height,
  };
}

export function btwKeyboardClearance(frame: BtwFloatingFrame, safeBottom: number): number {
  return Math.max(0, frame.bottom - safeBottom - EDGE);
}

function measureNativeAnchor(anchor: View): Promise<Rect> {
  return new Promise((resolve) => {
    // Both native roots are edge-to-edge. Use the anchor's root-relative coordinates
    // so Android's visible-window offset cannot leak into the dialog's coordinates.
    anchor.measure((_x, _y, width, height, pageX, pageY) => {
      resolve({ x: pageX, y: pageY, width, height });
    });
  });
}

export function useBtwFloatingLayout(
  visible: boolean,
  anchorRef: RefObject<View | null>,
  anchorLayoutVersion: number,
  modalSize: Size | null,
  keyboardLayoutShift: number,
): BtwFloatingLayout {
  const { width, height } = useWindowDimensions();
  const insets = useSafeAreaInsets();
  // Remeasure the composer after its own keyboard animation settles.
  const keyboardShift = useSettledKeyboardShift();
  const [measurement, setMeasurement] = useState<{ anchor: Rect; viewport: Viewport } | null>(null);

  useLayoutEffect(() => {
    if (!visible) {
      setMeasurement(null);
      return;
    }
    let disposed = false;
    let generation = 0;
    let raf = 0;
    let observed: View | null = null;
    let resize: ResizeObserver | undefined;
    let mutation: MutationObserver | undefined;
    const ancestors = new Set<Element>();
    const schedule = () => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(update);
    };
    const update = () => {
      const anchor = anchorRef.current;
      if (!anchor) return;
      if (isWeb && anchor !== observed && anchor instanceof HTMLElement) {
        resize?.disconnect();
        mutation?.disconnect();
        ancestors.clear();
        resize = typeof ResizeObserver === "undefined" ? undefined : new ResizeObserver(schedule);
        mutation =
          typeof MutationObserver === "undefined" ? undefined : new MutationObserver(schedule);
        for (let element: HTMLElement | null = anchor; element; element = element.parentElement) {
          ancestors.add(element);
          resize?.observe(element);
          mutation?.observe(element, { attributes: true, attributeFilter: ["style", "class"] });
        }
        observed = anchor;
      }
      const current = ++generation;
      const visual = isWeb ? window.visualViewport : null;
      const viewport = {
        x: visual?.offsetLeft ?? 0,
        y: visual?.offsetTop ?? 0,
        width: visual?.width ?? width,
        height: visual?.height ?? height,
        layoutHeight: height,
      };
      const measure = isWeb ? measureElement : measureNativeAnchor;
      void measure(anchor).then((rect) => {
        if (disposed || current !== generation) return;
        if (!Number.isFinite(rect.y)) return;
        setMeasurement({ anchor: rect, viewport });
      });
    };
    const onScroll = (event: Event) => {
      if (!(event.target instanceof Element) || ancestors.has(event.target)) schedule();
    };
    update();
    schedule();
    if (isWeb) {
      window.addEventListener("resize", schedule);
      document.addEventListener("scroll", onScroll, true);
      window.visualViewport?.addEventListener("resize", schedule);
      window.visualViewport?.addEventListener("scroll", schedule);
    }
    return () => {
      disposed = true;
      generation += 1;
      cancelAnimationFrame(raf);
      resize?.disconnect();
      mutation?.disconnect();
      if (isWeb) {
        window.removeEventListener("resize", schedule);
        document.removeEventListener("scroll", onScroll, true);
        window.visualViewport?.removeEventListener("resize", schedule);
        window.visualViewport?.removeEventListener("scroll", schedule);
      }
    };
  }, [visible, anchorRef, anchorLayoutVersion, width, height, keyboardShift, modalSize]);

  if (!measurement || (!isWeb && !modalSize)) {
    return { frame: null, dragBounds: null, layoutHeight: 0 };
  }
  const { anchor, viewport } = measurement;
  if (modalSize) {
    return resolveBtwNativeLayout(anchor, modalSize, insets, keyboardLayoutShift);
  }
  const frame = resolveBtwFloatingFrame(anchor, viewport, insets.top);
  return {
    frame,
    dragBounds: resolveBtwDragBounds(viewport, insets.top, frame),
    layoutHeight: viewport.layoutHeight,
  };
}

import { useLayoutEffect, useState, type RefObject } from "react";
import { useWindowDimensions, type View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { isWeb } from "@/constants/platform";
import { useSettledKeyboardShift } from "@/keyboard/shift";
import { measureElement, type Rect } from "@/components/ui/menu/menu-anchor";
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
  layoutHeight: number;
}

export function useBtwFloatingLayout(
  visible: boolean,
  anchorRef: RefObject<View | null>,
  anchorLayoutVersion: number,
): BtwFloatingLayout {
  const { width, height } = useWindowDimensions();
  const insets = useSafeAreaInsets();
  // The shared composer dock already applies the keyboard shift. Remeasure its
  // final screen coordinates rather than subtracting the keyboard inset twice.
  const keyboardShift = useSettledKeyboardShift();
  const [frame, setFrame] = useState<BtwFloatingFrame | null>(null);
  const [dragBounds, setDragBounds] = useState<BtwDragBounds | null>(null);

  useLayoutEffect(() => {
    if (!visible) {
      setFrame(null);
      setDragBounds(null);
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
      void measureElement(anchor).then((rect) => {
        if (disposed || current !== generation) return;
        const next = resolveBtwFloatingFrame(rect, viewport, insets.top);
        setFrame((previous) =>
          previous &&
          previous.left === next.left &&
          previous.bottom === next.bottom &&
          previous.width === next.width &&
          previous.height === next.height
            ? previous
            : next,
        );
        setDragBounds(
          resolveBtwDragBounds(viewport, insets.top, {
            width: next.width,
            height: next.height,
          }),
        );
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
  }, [visible, anchorRef, anchorLayoutVersion, width, height, insets.top, keyboardShift]);

  return { frame, dragBounds, layoutHeight: height };
}

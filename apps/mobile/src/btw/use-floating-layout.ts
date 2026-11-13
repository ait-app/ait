import { useLayoutEffect, useState, type RefObject } from "react";
import { useWindowDimensions, type View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { isWeb } from "@/constants/platform";
import { useSettledKeyboardShift } from "@/keyboard/shift";
import { measureElement, type Rect } from "@/components/ui/menu/menu-anchor";
import { SPACING } from "@/styles/theme";

interface Viewport extends Rect {
  layoutHeight: number;
}
export interface BtwFloatingFrame {
  left: number;
  bottom: number;
  width: number;
  height: number;
}

/** Keep the conversation entirely above its composer, including in narrow split panes. */
export function resolveBtwFloatingFrame(
  anchor: Rect,
  viewport: Viewport,
  safeTop: number,
): BtwFloatingFrame {
  const edge = SPACING[2];
  const top = viewport.y + safeTop + edge;
  const bottom = Math.min(anchor.y - SPACING[3], viewport.y + viewport.height - edge);
  const width = Math.max(0, Math.min(448, anchor.width, viewport.width - edge * 2));
  return {
    left: Math.max(
      viewport.x + edge,
      Math.min(anchor.x + anchor.width - width, viewport.x + viewport.width - edge - width),
    ),
    bottom: viewport.layoutHeight - bottom,
    width,
    height: Math.max(0, Math.min(560, bottom - top)),
  };
}

export function useBtwFloatingLayout(
  visible: boolean,
  anchorRef: RefObject<View | null>,
  anchorLayoutVersion: number,
): BtwFloatingFrame | null {
  const { width, height } = useWindowDimensions();
  const insets = useSafeAreaInsets();
  // The shared composer dock already applies the keyboard shift. Remeasure its
  // final screen coordinates rather than subtracting the keyboard inset twice.
  const keyboardShift = useSettledKeyboardShift();
  const [frame, setFrame] = useState<BtwFloatingFrame | null>(null);

  useLayoutEffect(() => {
    if (!visible) {
      setFrame(null);
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
  return frame;
}

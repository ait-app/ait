import { createElement, useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import type { ViewStyle } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { useAnimatedStyle, useSharedValue, type SharedValue } from "react-native-reanimated";
import {
  DEFAULT_IOS_KEYBOARD_INSET_MIN_HEIGHT,
  resolveVisualViewportKeyboardShift,
} from "./policy";
import { KeyboardShiftContext, SettledKeyboardShiftContext, useKeyboardShift } from "./context";

type KeyboardShiftMode = "translate" | "padding";

function isTextEntryElement(element: Element | null): boolean {
  if (!element) return false;
  if (element.tagName === "TEXTAREA") return true;
  if (element.tagName === "INPUT") {
    const type = (element as HTMLInputElement).type;
    return !["hidden", "button", "submit", "reset", "checkbox", "radio", "file"].includes(type);
  }
  return (
    element instanceof HTMLElement && (element.isContentEditable || element.role === "textbox")
  );
}

export function KeyboardShiftProvider({ children }: { children: ReactNode }) {
  const insets = useSafeAreaInsets();
  const shift = useSharedValue(0);
  const layoutShift = useSharedValue(0);
  const isMoving = useSharedValue(false);
  const bottomInset = useSharedValue(insets.bottom);
  const [settledShift, setSettledShift] = useState(0);
  const publishSettledShift = useCallback((nextShift: number) => {
    setSettledShift((currentShift) => (currentShift === nextShift ? currentShift : nextShift));
  }, []);

  useEffect(() => {
    bottomInset.value = insets.bottom;
  }, [bottomInset, insets.bottom]);

  useEffect(() => {
    let settledTimer: number | null = null;
    const updateShift = () => {
      const viewport = window.visualViewport;
      const nextShift = viewport
        ? resolveVisualViewportKeyboardShift({
            layoutHeight: window.innerHeight,
            viewportHeight: viewport.height,
            viewportOffsetTop: viewport.offsetTop,
            bottomInset: bottomInset.value,
            hasFocusedTextInput: isTextEntryElement(document.activeElement),
            minimumHeight: DEFAULT_IOS_KEYBOARD_INSET_MIN_HEIGHT,
          })
        : 0;

      if (shift.value !== nextShift) {
        isMoving.value = true;
        shift.value = nextShift;
        layoutShift.value = nextShift;
      }
      if (settledTimer !== null) window.clearTimeout(settledTimer);
      settledTimer = window.setTimeout(() => {
        isMoving.value = false;
        publishSettledShift(nextShift);
      }, 100);
    };

    const viewport = window.visualViewport;
    window.addEventListener("resize", updateShift);
    document.addEventListener("focusin", updateShift);
    document.addEventListener("focusout", updateShift);
    viewport?.addEventListener("resize", updateShift);
    viewport?.addEventListener("scroll", updateShift);
    updateShift();

    return () => {
      window.removeEventListener("resize", updateShift);
      document.removeEventListener("focusin", updateShift);
      document.removeEventListener("focusout", updateShift);
      viewport?.removeEventListener("resize", updateShift);
      viewport?.removeEventListener("scroll", updateShift);
      if (settledTimer !== null) window.clearTimeout(settledTimer);
    };
  }, [bottomInset, isMoving, layoutShift, publishSettledShift, shift]);

  const value = useMemo(
    () => ({ shift, layoutShift, isMoving, bottomInset }),
    [bottomInset, isMoving, layoutShift, shift],
  );

  return createElement(
    KeyboardShiftContext.Provider,
    { value },
    createElement(SettledKeyboardShiftContext.Provider, { value: settledShift }, children),
  );
}

export function useKeyboardShiftStyle(input: { mode: KeyboardShiftMode; enabled?: boolean }): {
  shift: SharedValue<number>;
  style: ReturnType<typeof useAnimatedStyle<ViewStyle>>;
} {
  const { shift, bottomInset } = useKeyboardShift();
  const mode = input.mode;
  const enabled = input.enabled ?? true;

  const style = useAnimatedStyle<ViewStyle>(() => {
    if (mode === "padding") {
      return { paddingBottom: enabled ? bottomInset.value + shift.value : 0 };
    }
    return { transform: [{ translateY: enabled ? -shift.value : 0 }] };
  }, [enabled, mode]);

  return { shift, style };
}

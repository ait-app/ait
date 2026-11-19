import {
  createContext,
  useCallback,
  useContext,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import { Modal, StyleSheet as NativeStyleSheet, View, type LayoutChangeEvent } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import type { View as RNView } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { useAnimatedReaction } from "react-native-reanimated";
import { scheduleOnRN } from "react-native-worklets";
import { btwKeyboardClearance, useBtwFloatingLayout } from "@/btw/use-floating-layout";
import { useBtwDrag, type BtwDragHandleProps } from "@/btw/use-floating-drag";
import type { Size } from "@/components/ui/menu/menu-anchor";
import { KeyboardTranslateView, useKeyboardShift } from "@/keyboard/shift";
import { inlineUnistylesStyle } from "@/styles/unistyles-inline-style";
import { isWeb } from "@/constants/platform";
import {
  getOverlayRoot,
  OverlayLayerProvider,
  useOverlayLayer,
  useWebOverlayRegistration,
} from "@/lib/overlay-root";
import { isImeComposingKeyboardEvent } from "@/utils/keyboard-ime";

const BtwDragHandleContext = createContext<BtwDragHandleProps | null>(null);

/** Props that turn a window header into the handle that moves the floating window. */
export function useBtwDragHandle(): BtwDragHandleProps {
  return useContext(BtwDragHandleContext) ?? {};
}

function BtwModalViewport({
  onLayout,
  onKeyboardLayoutShift,
  bottomClearance,
  children,
}: {
  onLayout: (event: LayoutChangeEvent) => void;
  onKeyboardLayoutShift: (shift: number) => void;
  bottomClearance: number;
  children: ReactNode;
}) {
  const { layoutShift } = useKeyboardShift();
  useAnimatedReaction(
    () => layoutShift.value,
    (next, previous) => {
      if (next !== previous) scheduleOnRN(onKeyboardLayoutShift, next);
    },
    [layoutShift, onKeyboardLayoutShift],
  );

  return (
    <View onLayout={onLayout} pointerEvents="box-none" style={styles.overlay}>
      <KeyboardTranslateView
        bottomClearance={bottomClearance}
        pointerEvents="box-none"
        style={NativeStyleSheet.absoluteFill}
      >
        {children}
      </KeyboardTranslateView>
    </View>
  );
}

export function BtwFloatingSurface({
  visible,
  onClose,
  onSubmit,
  children,
  anchorRef,
  anchorLayoutVersion = 0,
}: {
  visible: boolean;
  onClose: () => void;
  onSubmit: () => void;
  children: ReactNode;
  anchorRef: RefObject<RNView | null>;
  anchorLayoutVersion?: number;
}) {
  const [modalSize, setModalSize] = useState<Size | null>(null);
  const [keyboardLayoutShift, setKeyboardLayoutShift] = useState(0);
  const { bottom: safeBottom } = useSafeAreaInsets();
  useLayoutEffect(() => {
    if (visible) return;
    setModalSize(null);
    setKeyboardLayoutShift(0);
  }, [visible]);
  const onModalLayout = useCallback((event: LayoutChangeEvent) => {
    const { width, height } = event.nativeEvent.layout;
    if (width <= 0 || height <= 0) return;
    setModalSize((size) =>
      size?.width === width && size.height === height ? size : { width, height },
    );
  }, []);
  const layout = useBtwFloatingLayout(
    visible,
    anchorRef,
    anchorLayoutVersion,
    modalSize,
    keyboardLayoutShift,
  );
  const drag = useBtwDrag(layout);
  const frame = drag.frame;
  const layer = useOverlayLayer("floating");
  const elementRef = useRef<HTMLElement | null>(null);
  const setScope = useWebOverlayRegistration({
    active: isWeb && visible,
    layer,
    manageFocus: false,
    onKeyDown: (event) => {
      if (!(event.target instanceof Node) || !elementRef.current?.contains(event.target))
        return false;
      if (
        event.key === "Enter" &&
        !event.shiftKey &&
        event.target instanceof HTMLTextAreaElement &&
        !isImeComposingKeyboardEvent(event)
      ) {
        event.preventDefault();
        onSubmit();
        return true;
      }
      if (event.key !== "Escape") return false;
      event.preventDefault();
      onClose();
      return true;
    },
  });
  const assignScope = useCallback(
    (node: RNView | null) => {
      elementRef.current =
        typeof HTMLElement !== "undefined" && node instanceof HTMLElement ? node : null;
      setScope(node);
    },
    [setScope],
  );
  const dragHandle = useMemo(() => drag.headerProps, [drag.headerProps]);

  if (!visible) return null;
  const surface = frame && (
    <OverlayLayerProvider layer={layer}>
      <View pointerEvents="box-none" style={[styles.overlay, { zIndex: layer }]}>
        <View ref={assignScope} style={[styles.position, inlineUnistylesStyle(frame)]}>
          <BtwDragHandleContext.Provider value={dragHandle}>
            {children}
          </BtwDragHandleContext.Provider>
        </View>
      </View>
    </OverlayLayerProvider>
  );
  if (isWeb) {
    return frame && frame.height > 0 && frame.width > 0 && typeof document !== "undefined"
      ? createPortal(surface, getOverlayRoot())
      : null;
  }
  return (
    <Modal
      visible
      transparent
      statusBarTranslucent
      navigationBarTranslucent
      animationType="slide"
      onRequestClose={onClose}
    >
      <BtwModalViewport
        onLayout={onModalLayout}
        onKeyboardLayoutShift={setKeyboardLayoutShift}
        bottomClearance={frame ? btwKeyboardClearance(frame, safeBottom) : 0}
      >
        {surface}
      </BtwModalViewport>
    </Modal>
  );
}

const styles = StyleSheet.create({
  overlay: {
    position: "absolute",
    top: 0,
    left: 0,
    bottom: 0,
    right: 0,
  },
  position: { position: "absolute", pointerEvents: "auto" },
});

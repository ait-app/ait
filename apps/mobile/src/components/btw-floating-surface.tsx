import { useCallback, useRef, type ReactNode, type RefObject } from "react";
import { createPortal } from "react-dom";
import { Modal, View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { useBtwFloatingLayout } from "@/btw/use-floating-layout";
import { inlineUnistylesStyle } from "@/styles/unistyles-inline-style";
import { isWeb } from "@/constants/platform";
import {
  getOverlayRoot,
  OverlayLayerProvider,
  useOverlayLayer,
  useWebOverlayRegistration,
} from "@/lib/overlay-root";
import { isImeComposingKeyboardEvent } from "@/utils/keyboard-ime";

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
  anchorRef: RefObject<View | null>;
  anchorLayoutVersion?: number;
}) {
  const frame = useBtwFloatingLayout(visible, anchorRef, anchorLayoutVersion);
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
    (node: View | null) => {
      elementRef.current =
        typeof HTMLElement !== "undefined" && node instanceof HTMLElement ? node : null;
      setScope(node);
    },
    [setScope],
  );
  if (!visible || !frame || frame.height <= 0 || frame.width <= 0) return null;
  const surface = (
    <OverlayLayerProvider layer={layer}>
      <View pointerEvents="box-none" style={[styles.overlay, { zIndex: layer }]}>
        <View ref={assignScope} style={[styles.position, inlineUnistylesStyle(frame)]}>
          {children}
        </View>
      </View>
    </OverlayLayerProvider>
  );
  if (isWeb && typeof document !== "undefined") return createPortal(surface, getOverlayRoot());
  return (
    <Modal visible transparent animationType="slide" onRequestClose={onClose}>
      {surface}
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

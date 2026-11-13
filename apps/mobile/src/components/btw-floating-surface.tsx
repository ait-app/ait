import { useCallback, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { KeyboardAvoidingView, Modal, Platform, View, useWindowDimensions } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { SPACING } from "@/styles/theme";
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
}: {
  visible: boolean;
  onClose: () => void;
  onSubmit: () => void;
  children: ReactNode;
}) {
  const { width } = useWindowDimensions();
  const insets = useSafeAreaInsets();
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
  if (!visible) return null;
  const compact = width < 640;
  const surface = (
    <OverlayLayerProvider layer={layer}>
      <KeyboardAvoidingView
        pointerEvents="box-none"
        behavior={Platform.OS === "ios" ? "padding" : undefined}
        style={[styles.overlay, { zIndex: layer }]}
      >
        <View
          ref={assignScope}
          style={[
            styles.position,
            {
              width: compact ? Math.max(0, width - SPACING[2] * 2) : 448,
              marginRight: compact ? SPACING[2] : SPACING[6],
              marginBottom: compact ? Math.max(insets.bottom, SPACING[2]) : SPACING[6],
            },
          ]}
        >
          {children}
        </View>
      </KeyboardAvoidingView>
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
    alignItems: "flex-end",
    justifyContent: "flex-end",
  },
  position: { pointerEvents: "auto" },
});

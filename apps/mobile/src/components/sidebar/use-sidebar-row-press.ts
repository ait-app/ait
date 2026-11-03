import { useCallback, useState } from "react";
import { isNative } from "@/constants/platform";

export function useSidebarRowPress() {
  const [isPressed, setIsPressed] = useState(false);
  const onPressIn = useCallback(() => setIsPressed(true), []);
  const onPressOut = useCallback(() => setIsPressed(false), []);

  // Native PressHighlight owns feedback through the gesture's UI-thread finalizer.
  // A second background driven by Pressable callbacks can outlive that gesture when
  // scrolling takes over the touch, leaving the row highlighted after release.
  return {
    isPressed: !isNative && isPressed,
    onPressIn: isNative ? undefined : onPressIn,
    onPressOut: isNative ? undefined : onPressOut,
  };
}

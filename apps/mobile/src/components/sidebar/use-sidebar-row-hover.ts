import { useCallback, useEffect, useMemo, useState } from "react";
import type { PointerEvent } from "react-native";
import { useIsCompactFormFactor } from "@/constants/layout";

export function useSidebarRowHover(disabled = false) {
  const isCompact = useIsCompactFormFactor();
  const enabled = !isCompact && !disabled;
  const [hovered, setHovered] = useState(false);

  useEffect(() => {
    if (!enabled) setHovered(false);
  }, [enabled]);

  const onPointerEnter = useCallback(
    (event: PointerEvent) => {
      // Fingers also emit pointer-enter events, without a mouse-style leave on every row.
      setHovered(enabled && event.nativeEvent.pointerType === "mouse");
    },
    [enabled],
  );
  const onPointerLeave = useCallback(() => setHovered(false), []);
  const hoverHandlers = useMemo(
    () => ({ onPointerEnter, onPointerLeave }),
    [onPointerEnter, onPointerLeave],
  );

  return { isHovered: enabled && hovered, hoverHandlers };
}

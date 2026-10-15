import { useEffect, type RefObject } from "react";
import { BackHandler, Platform } from "react-native";
import type { SharedValue } from "react-native-reanimated";
import { usePanelStore } from "@/stores/panel-store";
import { subscribeSystemPanelBack, type SystemPanelBackEvent } from "./system-back-source";

export function useSystemPanelBack({
  enabled,
  fileExplorerOpener,
  openGesturesBlocked,
  scrolledRight,
}: {
  enabled: boolean;
  fileExplorerOpener: RefObject<(() => void) | null>;
  openGesturesBlocked: SharedValue<boolean>;
  scrolledRight?: SharedValue<boolean>;
}) {
  useEffect(() => {
    if (!enabled || Platform.OS !== "android") return;

    let edge: SystemPanelBackEvent["edge"] = null;
    const unsubscribe = subscribeSystemPanelBack((event) => {
      edge = event.edge;
    });
    const subscription = BackHandler.addEventListener("hardwareBackPress", () => {
      const currentEdge = edge;
      edge = null;
      const state = usePanelStore.getState();
      const panel = state.mobilePanel.target;

      if (currentEdge === null) {
        if (panel === "agent") return false;
        state.showMobileAgent();
        return true;
      }

      if (panel !== "agent") {
        if (
          (panel === "agent-list" && currentEdge === "right") ||
          (panel === "file-explorer" && currentEdge === "left")
        )
          state.showMobileAgent();
        return true;
      }

      // The system owns these touches, so consuming a blocked swipe must still prevent
      // the Activity's default Back action from sending the user to the launcher.
      if (openGesturesBlocked.value) return true;
      if (currentEdge === "left") {
        if (!scrolledRight?.value) state.showMobileAgentList();
      } else {
        fileExplorerOpener.current?.();
      }
      return true;
    });
    return () => {
      subscription.remove();
      unsubscribe();
    };
  }, [enabled, fileExplorerOpener, openGesturesBlocked, scrolledRight]);
}

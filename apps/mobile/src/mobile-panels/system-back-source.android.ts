import { requireOptionalNativeModule, type EventSubscription } from "expo-modules-core";
import type { SystemPanelBackEvent } from "./system-back-source";

interface PanelGesturesModule {
  setEnabled(enabled: boolean): void;
  addListener(
    event: "backGesture",
    listener: (event: SystemPanelBackEvent) => void,
  ): EventSubscription;
}

export function subscribeSystemPanelBack(listener: (event: SystemPanelBackEvent) => void) {
  const native = requireOptionalNativeModule<PanelGesturesModule>("AitPanelGestures");
  // Older development clients need rebuilding to include the edge metadata adapter.
  if (!native) return () => {};
  const subscription = native.addListener("backGesture", listener);
  native.setEnabled(true);
  return () => {
    native.setEnabled(false);
    subscription.remove();
  };
}

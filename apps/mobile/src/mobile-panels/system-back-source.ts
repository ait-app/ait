export interface SystemPanelBackEvent {
  edge: "left" | "right" | null;
}

export function subscribeSystemPanelBack(_listener: (event: SystemPanelBackEvent) => void) {
  return () => {};
}

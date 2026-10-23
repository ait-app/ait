import type { RevokePushNotificationsInput, StartPushNotificationsInput } from "./internal/types";

export function startPushNotifications(_input: StartPushNotificationsInput): () => void {
  return () => undefined;
}

export async function revokePushNotifications(_input: RevokePushNotificationsInput): Promise<void> {
  // Push notifications are native-only.
}

export function startAccountPushNotifications(_input: {
  connection: import("@ait/client/internal/hub").HubAccountConnection;
  key: string;
}): () => void {
  return () => {};
}
export async function revokeAccountPushNotifications(): Promise<void> {}

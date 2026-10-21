import * as Notifications from "expo-notifications";
import { AppState, Platform } from "react-native";

let permissionRequest: Promise<boolean> | null = null;

export function isPushPermissionGranted(
  permission: Notifications.NotificationPermissionsStatus,
): boolean {
  return (
    permission.granted ||
    permission.status === Notifications.PermissionStatus.GRANTED ||
    permission.ios?.status === Notifications.IosAuthorizationStatus.PROVISIONAL ||
    permission.ios?.status === Notifications.IosAuthorizationStatus.EPHEMERAL
  );
}

export function ensureNativeNotificationPermission(): Promise<boolean> {
  if (permissionRequest) return permissionRequest;
  permissionRequest = requestPermission().finally(() => {
    permissionRequest = null;
  });
  return permissionRequest;
}

async function requestPermission(): Promise<boolean> {
  if (Platform.OS === "android") {
    await Notifications.setNotificationChannelAsync("default", {
      name: "default",
      importance: Notifications.AndroidImportance.DEFAULT,
    });
  }
  const existing = await Notifications.getPermissionsAsync();
  if (isPushPermissionGranted(existing)) return true;
  if (!existing.canAskAgain || AppState.currentState !== "active") return false;
  return isPushPermissionGranted(
    await Notifications.requestPermissionsAsync({
      ios: { allowAlert: true, allowBadge: true, allowSound: true },
    }),
  );
}

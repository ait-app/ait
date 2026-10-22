import AsyncStorage from "@react-native-async-storage/async-storage";
import * as Application from "expo-application";
import * as Notifications from "expo-notifications";
import { AppState, Platform } from "react-native";
import { z } from "zod";
import { readValidatedString } from "@/storage/validated-storage";
import type { RevokePushNotificationsInput } from "./types";
import { ensureNativeNotificationPermission } from "./permissions";

const STORAGE_PREFIX = "@ait:native-push-token:";
const PushTokenSchema = z.string().trim().min(1).max(4096);
const refreshListeners = new Set<() => void>();

interface PushClient {
  readonly isConnected: boolean;
  registerPushToken(token: string): void | Promise<void>;
  unregisterPushToken(token: string): Promise<void>;
  subscribeConnectionStatus(listener: (state: { status: string }) => void): () => void;
}

export function refreshPushRegistrations(): void {
  for (const refresh of refreshListeners) refresh();
}

function storageKey(serverId: string): string {
  return `${STORAGE_PREFIX}${serverId}`;
}

async function resolveToken(
  serverId: string,
  changedToken?: Notifications.DevicePushToken,
): Promise<string | null> {
  const key = storageKey(serverId);
  // Permission must not depend on the token cache being readable.
  if (!(await ensureNativeNotificationPermission())) {
    await AsyncStorage.removeItem(key).catch(() => undefined);
    return null;
  }
  const cached = await readValidatedString(AsyncStorage, key, PushTokenSchema).catch(() => null);
  try {
    const result = changedToken ?? (await Notifications.getDevicePushTokenAsync());
    const deviceToken = typeof result.data === "string" ? result.data.trim() : "";
    if (!deviceToken) return cached;
    let token: string;
    if (Platform.OS === "ios") {
      const environment = await Application.getIosPushNotificationServiceEnvironmentAsync();
      if (!Application.applicationId || !environment) {
        console.warn("[PushNotifications] APNs entitlement or application ID is unavailable");
        return null;
      }
      token = JSON.stringify({
        provider: "apns",
        token: deviceToken,
        bundleId: Application.applicationId,
        environment,
      });
    } else {
      token = JSON.stringify({ provider: "fcm", token: deviceToken });
    }
    await AsyncStorage.setItem(key, token).catch(() => {
      console.warn("[PushNotifications] Push token cache write failed");
    });
    return token;
  } catch {
    console.warn("[PushNotifications] Native push token lookup failed");
    if (cached) return cached;
    throw new Error("Native push token is unavailable");
  }
}

export function startSubscription(input: { client: PushClient; serverId: string }): () => void {
  let stopped = false;
  let token: string | null = null;
  let pending = false;
  let queuedToken: Notifications.DevicePushToken | undefined;
  let retry: ReturnType<typeof setTimeout> | undefined;
  const register = () => {
    if (!stopped && token && input.client.isConnected) {
      void Promise.resolve(input.client.registerPushToken(token)).catch(() => {
        if (!stopped) retry = setTimeout(refresh, 30_000);
      });
    }
  };

  const refresh = (changedToken?: Notifications.DevicePushToken) => {
    if (changedToken) queuedToken = changedToken;
    if (stopped || pending || AppState.currentState !== "active") return;
    clearTimeout(retry);
    pending = true;
    const candidate = queuedToken;
    queuedToken = undefined;
    void resolveToken(input.serverId, candidate)
      .then((resolved) => {
        if (stopped) return;
        const previous = token;
        token = resolved;
        if (previous && previous !== token && input.client.isConnected) {
          void input.client.unregisterPushToken(previous).catch(() => undefined);
        }
        register();
      })
      .catch(() => {
        if (!stopped) {
          console.warn("[PushNotifications] Registration failed; retrying while active");
          retry = setTimeout(refresh, 30_000);
        }
      })
      .finally(() => {
        pending = false;
        if (queuedToken && !stopped) refresh();
      });
  };
  refreshListeners.add(refresh);
  refresh();
  const appState = AppState.addEventListener("change", (state) => {
    if (state === "active") refresh();
  });
  const tokenChanges = Notifications.addPushTokenListener((changedToken) => refresh(changedToken));

  const unsubscribe = input.client.subscribeConnectionStatus((state) => {
    if (state.status === "connected") {
      if (token) register();
      else refresh();
    }
  });

  return () => {
    stopped = true;
    clearTimeout(retry);
    refreshListeners.delete(refresh);
    appState.remove();
    tokenChanges.remove();
    unsubscribe();
  };
}

export async function revokeSubscription(input: RevokePushNotificationsInput): Promise<void> {
  const key = storageKey(input.serverId);
  const token = await readValidatedString(AsyncStorage, key, PushTokenSchema);
  if (
    token &&
    input.client?.isConnected &&
    input.client.getLastServerInfoMessage()?.features?.pushTokenRevocation === true
  ) {
    try {
      await input.client.unregisterPushToken(token);
    } catch {
      console.warn("[PushNotifications] Failed to revoke push token");
    }
  }
  await AsyncStorage.removeItem(key);
}

import { AppState, Platform } from "react-native";
import * as Crypto from "expo-crypto";
import { fetch as nativeFetch } from "expo/fetch";
import * as SecureStore from "expo-secure-store";
import AsyncStorage from "@react-native-async-storage/async-storage";
import {
  AccountSessionManager,
  normalizeCenter,
  type AccountSnapshot,
  type SavedAccount,
} from "@ait/client/internal/account-session";
import { resolveAppVersion } from "@/utils/app-version";
import { androidBrowserLogin } from "./account-browser-login.native";

const INSTALLATION_KEY = "@ait:account-installation-v1";
const CREDENTIAL_KEY = "ait.account.session.v1";
const listeners = new Set<(state: AccountSnapshot) => void>();
const transports = new Set<() => void>();
let ready: Promise<AccountSessionManager> | undefined;

async function createAccount(): Promise<AccountSessionManager> {
  const platform = Platform.OS === "ios" ? "ios" : "android";
  let installationId = await AsyncStorage.getItem(INSTALLATION_KEY);
  if (!installationId || !/^[0-9a-f-]{36}$/i.test(installationId)) {
    installationId = Crypto.randomUUID();
    await AsyncStorage.setItem(INSTALLATION_KEY, installationId);
  }
  const manager = new AccountSessionManager({
    installationId,
    deviceName: platform === "ios" ? "Ait iOS" : "Ait Android",
    platform,
    browserLogin: platform === "android" ? androidBrowserLogin : undefined,
    randomUUID: Crypto.randomUUID,
    fetch: nativeFetch as typeof fetch,
    appVersion: resolveAppVersion() ?? "unknown",
    runtime: () => ({ status: "stopped", serverId: "" }),
    local: async () => undefined,
    save: async (account) => {
      if (account) await SecureStore.setItemAsync(CREDENTIAL_KEY, JSON.stringify(account));
      else await SecureStore.deleteItemAsync(CREDENTIAL_KEY);
    },
    notify: (state) => {
      for (const listener of listeners) listener(state);
    },
    closeTransports: () => {
      // Closing removes entries and may synchronously create replacement connections.
      // oxlint-disable-next-line unicorn/no-useless-spread
      for (const close of [...transports]) close();
    },
  });
  if (AppState.currentState !== "active") manager.suspend();
  const saved = await SecureStore.getItemAsync(CREDENTIAL_KEY);
  if (saved) {
    let account: SavedAccount | undefined;
    try {
      const value: unknown = JSON.parse(saved);
      if (
        value &&
        typeof value === "object" &&
        "token" in value &&
        typeof value.token === "string" &&
        "center" in value &&
        typeof value.center === "string" &&
        "name" in value &&
        typeof value.name === "string" &&
        "expiresAt" in value &&
        typeof value.expiresAt === "number" &&
        value.expiresAt > Date.now() &&
        (!("nodeSessionId" in value) || typeof value.nodeSessionId === "string")
      ) {
        account = { ...(value as SavedAccount), center: normalizeCenter(value.center) };
      }
    } catch {
      /* Invalid saved credentials require a fresh login. */
    }
    if (account) await manager.restore(account);
    else await SecureStore.deleteItemAsync(CREDENTIAL_KEY);
  }
  // This manager is a process singleton; keep exactly one native lifecycle subscription.
  AppState.addEventListener("change", (state) => {
    if (state === "active") manager.resume();
    else manager.suspend();
  });
  return manager;
}

export function getNativeAccount(): Promise<AccountSessionManager> {
  ready ??= createAccount().catch((error) => {
    ready = undefined;
    throw error;
  });
  return ready;
}

export function subscribeNativeAccount(listener: (state: AccountSnapshot) => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function registerNativeAccountTransport(close: () => void): () => void {
  if (transports.size >= 16) throw new Error("Too many account relay connections.");
  transports.add(close);
  return () => {
    transports.delete(close);
  };
}

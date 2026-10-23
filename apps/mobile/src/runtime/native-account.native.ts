import {
  createWebSocketTransportFactory,
  nativeWebSocketFactory,
} from "@ait/client/internal/daemon-client-websocket-transport";
import { HubLink } from "@ait/client/internal/hub";
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
import { androidBrowserLogin, iosBrowserLogin } from "./account-browser-login.native";

const INSTALLATION_KEY = "@ait:account-installation-v1";
const CREDENTIAL_KEY = "ait.account.session.v1";
const BACKGROUND_CONNECTION_GRACE_MS = 2 * 60 * 1000;
const listeners = new Set<(state: AccountSnapshot) => void>();
const transports = new Set<() => void>();
let ready: Promise<AccountSessionManager> | undefined;
let hub: HubLink | null = null;

async function createAccount(): Promise<AccountSessionManager> {
  const platform = Platform.OS === "ios" ? "ios" : "android";
  let installationId = await AsyncStorage.getItem(INSTALLATION_KEY);
  if (!installationId || !/^[0-9a-f-]{36}$/i.test(installationId)) {
    installationId = Crypto.randomUUID();
    await AsyncStorage.setItem(INSTALLATION_KEY, installationId);
  }
  // One access link per signed-in account; it pauses with the account while backgrounded.
  let hubPaused = AppState.currentState === "background";
  const link = new HubLink({
    ticket: () => manager.openHubTicket(),
    webSocketFactory: nativeWebSocketFactory,
    authentication: "bearer",
    randomUUID: Crypto.randomUUID,
  });
  const syncHub = (status: AccountSnapshot["status"]) => {
    if (status === "logged_out" || hubPaused) link.stop();
    else link.start();
  };
  const manager = new AccountSessionManager({
    installationId,
    channelTransport: createWebSocketTransportFactory(nativeWebSocketFactory),
    channelAuthentication: "bearer",
    deviceName: platform === "ios" ? "Ait iOS" : "Ait Android",
    platform,
    browserLogin: platform === "ios" ? iosBrowserLogin : androidBrowserLogin,
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
      syncHub(state.status);
      for (const listener of listeners) listener(state);
    },
    closeTransports: () => {
      // Closing removes entries and may synchronously create replacement connections.
      // oxlint-disable-next-line unicorn/no-useless-spread
      for (const close of [...transports]) close();
    },
  });
  manager.attachHub(link);
  // A cold background launch has no live connection to retain.
  if (AppState.currentState === "background") manager.suspend();
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
  let backgroundTimer: ReturnType<typeof setTimeout> | undefined;
  const onAppState = (state: string) => {
    if (state === "active") {
      clearTimeout(backgroundTimer);
      backgroundTimer = undefined;
      hubPaused = false;
      manager.resume();
      syncHub(manager.snapshot().status);
      // The OS may have silently dropped the socket while backgrounded.
      void link.verify();
    } else if (state === "background" && backgroundTimer === undefined) {
      // Brief app switches retain relay sockets and in-progress downloads. An
      // inactive transition alone (e.g. a system dialog) must not disconnect them.
      backgroundTimer = setTimeout(() => {
        // Keep the timer marker until foregrounding so repeated background
        // events cannot restart the grace period or suspend the account twice.
        hubPaused = true;
        manager.suspend();
      }, BACKGROUND_CONNECTION_GRACE_MS);
    }
  };
  AppState.addEventListener("change", onAppState);
  // Reconcile transitions that happened while restoring saved credentials.
  onAppState(AppState.currentState);
  hub = link;
  return manager;
}

export function getNativeAccount(): Promise<AccountSessionManager> {
  ready ??= createAccount().catch((error) => {
    ready = undefined;
    throw error;
  });
  return ready;
}

/** The signed-in account's hub link; pending while the account is still being restored. */
export function getNativeHubLink(): HubLink | Promise<HubLink | null> {
  return (
    hub ??
    getNativeAccount().then(
      () => hub,
      () => null,
    )
  );
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

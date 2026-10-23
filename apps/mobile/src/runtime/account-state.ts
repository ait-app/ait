import { useEffect } from "react";
import { Platform } from "react-native";
import { create } from "zustand";
import { getDesktopHost } from "@/desktop/host";
import { getHostRuntimeStore } from "./host-runtime";
import { markStartupPhase } from "./startup-timeline";
import { getNativeAccount, subscribeNativeAccount } from "./native-account";
import {
  supportsBrowserAccount,
  getBrowserAccount,
  subscribeBrowserAccount,
} from "./browser-account";
import { DEFAULT_ACCOUNT_CENTER } from "@ait/client/internal/account-session";
import { revokeAccountPushNotifications } from "@/push-notifications";

import type { AccountHost, AccountInfo } from "@ait/client/internal/account-session";
export type { AccountHost } from "@ait/client/internal/account-session";

export interface AccountState {
  status: "logged_out" | "connecting" | "online" | "error";
  center: string;
  name: string;
  hostOnline: boolean;
  hosts: AccountHost[];
  stale: boolean;
  confirmedHostIds?: string[];
  error: string | null;
  selected: AccountHost | null;
  synchronizedHosts?: string[];
  /** Desktop-owned preference, retained when the built-in daemon is manually taken offline. */
  syncBuiltInDaemon?: boolean;
  accountExpiresAt?: string | null;
  loginPending?: boolean;
  accountId?: string;
}

/** Account hosts offline longer than this leave the host list until they report again. */
export const ACCOUNT_HOST_OFFLINE_RETENTION_MS = 24 * 60 * 60 * 1000;

export const useAccountState = create<AccountState>(() => ({
  status: "logged_out",
  center:
    supportsBrowserAccount() && typeof window !== "undefined"
      ? `${window.location.origin}/api`
      : DEFAULT_ACCOUNT_CENTER,
  name: "",
  hostOnline: false,
  hosts: [],
  stale: true,
  error: null,
  selected: null,
}));

export function supportsAccountRelay(): boolean {
  return (
    Platform.OS === "android" ||
    Platform.OS === "ios" ||
    Boolean(getDesktopHost()?.invoke) ||
    supportsBrowserAccount()
  );
}

/** Read account details through the platform that owns the signed-in credentials. */
export async function loadAccountInfo(): Promise<AccountInfo> {
  const invoke = getDesktopHost()?.invoke;
  if (invoke) return (await invoke("account_info")) as AccountInfo;
  const manager = await (supportsBrowserAccount() ? getBrowserAccount() : getNativeAccount());
  return manager.getAccountInfo();
}

let commandTail = Promise.resolve<unknown>(undefined);
let snapshotSequence = 0;

/** Account and host publication changes share one native authority queue. */
export function serializeNativeAccountCommand<T>(work: () => Promise<T>): Promise<T> {
  const operation = commandTail.then(work);
  commandTail = operation.catch(() => undefined);
  return operation;
}

async function receiveAccountSnapshot(
  snapshot: AccountState,
  isActive?: () => boolean,
): Promise<void> {
  useAccountState.setState(snapshot);
  if (snapshot.status === "online") {
    markStartupPhase("node_registered");
    if (!snapshot.stale) markStartupPhase("hosts_listed");
  }
  const sequence = ++snapshotSequence;
  const store = getHostRuntimeStore();
  // Only the saved registry is needed here; boot() also waits for a localhost probe.
  await store.loadRegistry();
  const current = () => sequence === snapshotSequence && (isActive?.() ?? true);
  if (!current()) return;
  const active = snapshot.status === "online" || snapshot.status === "connecting";
  const confirmed = new Set(snapshot.confirmedHostIds ?? []);
  const freshHosts = active
    ? snapshot.hosts.filter(
        (host) => (snapshot.status === "online" && !snapshot.stale) || confirmed.has(host.host_id),
      )
    : [];
  const boundHosts = freshHosts.filter((host): host is AccountHost & { server_id: string } =>
    Boolean(host.server_id),
  );
  const routableHosts = boundHosts.filter(
    (host) =>
      host.online &&
      (host.hub_online || (host.instance_id && host.relay_modes.includes("ait-rust-single-v1"))),
  );
  if (snapshot.status === "online" || (active && freshHosts.length > 0)) {
    store.setAccountRelayCenter(snapshot.center);
    // Only a complete host list may prune saved hosts; stale lists may omit live ones.
    const complete = snapshot.status === "online" && !snapshot.stale;
    const now = Date.now();
    const retainedHosts = complete
      ? boundHosts.filter((host) => {
          const offlineSince = host.offline_since ? Date.parse(host.offline_since) : NaN;
          return (
            host.online ||
            !Number.isFinite(offlineSince) ||
            now - offlineSince < ACCOUNT_HOST_OFFLINE_RETENTION_MS
          );
        })
      : boundHosts;
    await store.addDiscoveredAccountRelayHosts(retainedHosts, snapshot.center, current, {
      ...(snapshot.accountId ? { accountId: snapshot.accountId } : {}),
      ...(complete ? { retainedHostIds: new Set(retainedHosts.map((host) => host.host_id)) } : {}),
    });
  } else if (snapshot.status === "logged_out") store.setAccountRelayCenter(null);
  if (current()) {
    const available = routableHosts.map((host) => host.server_id);
    if (snapshot.status !== "logged_out" && snapshot.stale) {
      // A status-channel reconnect does not invalidate healthy data connections.
      // Explicitly confirmed offline events still take effect immediately.
      store.setAvailableAccountRelayHosts(
        available,
        snapshot.hosts.flatMap((host) => (!host.online && host.server_id ? [host.server_id] : [])),
      );
    } else store.setAvailableAccountRelayHosts(available);
  }
}

async function persistAccountHostSelection(
  command: string,
  args: Record<string, unknown> | undefined,
  snapshot: AccountState,
  previousServerId: string | null,
): Promise<void> {
  if (command !== "account_select") return;
  const store = getHostRuntimeStore();
  await store.loadRegistry();
  if (typeof args?.hostId === "string" && snapshot.selected?.server_id) {
    await store.addAccountRelayHost(
      { ...snapshot.selected, server_id: snapshot.selected.server_id },
      snapshot.center,
    );
  } else if (previousServerId) {
    const host = store.getHosts().find((host) => host.serverId === previousServerId);
    for (const connection of host?.connections ?? []) {
      if (connection.type === "accountRelay" && connection.center === snapshot.center) {
        await store.removeConnection(previousServerId, connection.id);
      }
    }
  }
}

export async function accountCommand(
  command: string,
  args?: Record<string, unknown>,
): Promise<AccountState> {
  const invoke = getDesktopHost()?.invoke;
  if (!invoke && (Platform.OS === "android" || Platform.OS === "ios" || supportsBrowserAccount())) {
    // Cancellation must not queue behind the browser operation it is cancelling.
    if (command === "account_cancel_login") {
      const manager = await (supportsBrowserAccount() ? getBrowserAccount() : getNativeAccount());
      manager.cancelLogin();
      return manager.snapshot();
    }
    return serializeNativeAccountCommand(async () => {
      const previousServerId = useAccountState.getState().selected?.server_id ?? null;
      const manager = await (supportsBrowserAccount() ? getBrowserAccount() : getNativeAccount());
      switch (command) {
        case "account_login_hosted":
          if (args?.center !== undefined && typeof args.center !== "string")
            throw new Error("Invalid service URL.");
          await manager.loginWithBrowser((args?.center as string) ?? "");
          break;
        case "account_logout":
          await revokeAccountPushNotifications();
          await manager.logout();
          break;
        case "account_select":
          await manager.select(typeof args?.hostId === "string" ? args.hostId : null);
          break;
        case "account_refresh":
          manager.refresh();
          break;
        case "account_status":
          break;
        default:
          throw new Error("Unknown account command.");
      }
      const snapshot = manager.snapshot();
      await receiveAccountSnapshot(snapshot);
      await persistAccountHostSelection(command, args, snapshot, previousServerId);
      return snapshot;
    });
  }
  if (!invoke) throw new Error("Account login is available in the native mobile and desktop apps.");
  if (command === "account_cancel_login") return (await invoke(command, args)) as AccountState;
  return serializeNativeAccountCommand(async () => {
    const previousServerId = useAccountState.getState().selected?.server_id ?? null;
    const snapshot = (await invoke(command, args)) as AccountState;
    await receiveAccountSnapshot(snapshot);
    await persistAccountHostSelection(command, args, snapshot, previousServerId);
    return snapshot;
  });
}

/** Mount once next to HostRuntime bootstrap to import all discovered account hosts. */
export function AccountRelayLifecycle() {
  useEffect(() => {
    if (
      !getDesktopHost()?.invoke &&
      (Platform.OS === "android" || Platform.OS === "ios" || supportsBrowserAccount())
    ) {
      let disposed = false;
      const receive = (snapshot: AccountState) => {
        if (!disposed)
          void receiveAccountSnapshot(snapshot, () => !disposed).catch(() => {
            if (!disposed)
              useAccountState.setState({ error: "Could not synchronize online hosts." });
          });
      };
      const remove = (supportsBrowserAccount() ? subscribeBrowserAccount : subscribeNativeAccount)(
        receive,
      );
      void (supportsBrowserAccount() ? getBrowserAccount() : getNativeAccount())
        .then((manager) => {
          markStartupPhase("account_restored");
          receive(manager.snapshot());
        })
        .catch(() => {
          if (!disposed)
            useAccountState.setState({
              error: "Could not restore the account. Please sign in again.",
            });
        });
      return () => {
        disposed = true;
        remove();
      };
    }
    const desktop = getDesktopHost();
    if (!desktop?.invoke || !desktop.events?.on) return;
    let disposed = false;
    let sequence = 0;
    let remove: (() => void) | undefined;
    const receive = (value: unknown) => {
      if (disposed) return;
      ++sequence;
      void receiveAccountSnapshot(value as AccountState, () => !disposed).catch(() => {
        if (!disposed) useAccountState.setState({ error: "Could not synchronize online hosts." });
      });
    };
    void (async () => {
      const unsubscribe = await desktop.events!.on!("account-state", receive);
      if (disposed) {
        unsubscribe();
        return;
      }
      remove = unsubscribe;
      const current = sequence;
      const snapshot = await desktop.invoke!("account_status");
      if (sequence === current) receive(snapshot);
    })().catch(() => undefined);
    const refresh = () => {
      void desktop.invoke!("account_refresh").catch(() => undefined);
    };
    window.addEventListener("online", refresh);
    window.addEventListener("focus", refresh);
    return () => {
      disposed = true;
      remove?.();
      window.removeEventListener("online", refresh);
      window.removeEventListener("focus", refresh);
    };
  }, []);
  return null;
}

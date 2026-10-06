import { useEffect } from "react";
import { Platform } from "react-native";
import { create } from "zustand";
import { getDesktopHost } from "@/desktop/host";
import { getHostRuntimeStore } from "./host-runtime";
import { getNativeAccount, subscribeNativeAccount } from "./native-account";
import { DEFAULT_ACCOUNT_CENTER } from "@ait/client/internal/account-session";

export interface AccountHost {
  host_id: string;
  node_id: string;
  server_id: string;
  instance_id: string;
  name: string;
  platform: string;
  relay_modes: string[];
}
export interface AccountState {
  status: "logged_out" | "connecting" | "online" | "error";
  center: string;
  name: string;
  hostOnline: boolean;
  hosts: AccountHost[];
  stale: boolean;
  error: string | null;
  selected: AccountHost | null;
  synchronizedHosts?: string[];
  accountExpiresAt?: string | null;
  loginPending?: boolean;
}

export const useAccountState = create<AccountState>(() => ({
  status: "logged_out",
  center: DEFAULT_ACCOUNT_CENTER,
  name: "",
  hostOnline: false,
  hosts: [],
  stale: true,
  error: null,
  selected: null,
}));

export function supportsAccountRelay(): boolean {
  return Platform.OS === "android" || Platform.OS === "ios" || Boolean(getDesktopHost()?.invoke);
}

let commandTail = Promise.resolve<unknown>(undefined);
let snapshotSequence = 0;

/** Account and host publication changes share one native authority queue. */
export function serializeNativeAccountCommand<T>(work: () => Promise<T>): Promise<T> {
  const operation = commandTail.then(work);
  commandTail = operation.catch(() => undefined);
  return operation;
}

async function receiveAccountSnapshot(snapshot: AccountState): Promise<void> {
  useAccountState.setState(snapshot);
  const sequence = ++snapshotSequence;
  const store = getHostRuntimeStore();
  await store.boot();
  if (sequence === snapshotSequence) store.setAccountRelayHost(snapshot.selected);
}

export async function accountCommand(
  command: string,
  args?: Record<string, unknown>,
): Promise<AccountState> {
  const invoke = getDesktopHost()?.invoke;
  if (!invoke && (Platform.OS === "android" || Platform.OS === "ios")) {
    // Cancellation must not queue behind the browser operation it is cancelling.
    if (command === "account_cancel_login") {
      const manager = await getNativeAccount();
      manager.cancelLogin();
      return manager.snapshot();
    }
    return serializeNativeAccountCommand(async () => {
      const manager = await getNativeAccount();
      switch (command) {
        case "account_login_hosted":
          if (args?.center !== undefined && typeof args.center !== "string")
            throw new Error("Invalid service URL.");
          await manager.loginWithBrowser((args?.center as string) ?? "");
          break;
        case "account_login":
          if (
            typeof args?.email !== "string" ||
            typeof args.password !== "string" ||
            (args.center !== undefined && typeof args.center !== "string")
          )
            throw new Error("Invalid login.");
          await manager.login(args.center ?? "", args.email, args.password);
          break;
        case "account_logout":
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
      return snapshot;
    });
  }
  if (!invoke) throw new Error("Account login is available in the native mobile and desktop apps.");
  const snapshot = (await invoke(command, args)) as AccountState;
  useAccountState.setState(snapshot);
  return snapshot;
}

/** Provider discovery returns public capabilities, never credentials. */
export async function accountLoginMethods(center: string): Promise<{ hosted: boolean }> {
  const invoke = getDesktopHost()?.invoke;
  if (invoke) return (await invoke("account_login_methods", { center })) as { hosted: boolean };
  if (Platform.OS === "android") return (await getNativeAccount()).loginMethods(center);
  return { hosted: false };
}

/** Mount once next to HostRuntime bootstrap; discovery remains separate from runtime hosts. */
export function AccountRelayLifecycle() {
  useEffect(() => {
    if (Platform.OS === "android" || Platform.OS === "ios") {
      let disposed = false;
      const receive = (snapshot: AccountState) => {
        if (!disposed)
          void receiveAccountSnapshot(snapshot).catch(() => {
            if (!disposed)
              useAccountState.setState({ error: "Could not connect to the selected host." });
          });
      };
      const remove = subscribeNativeAccount(receive);
      void getNativeAccount()
        .then((manager) => receive(manager.snapshot()))
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
      const snapshot = value as AccountState;
      useAccountState.setState(snapshot);
      const current = ++sequence;
      const store = getHostRuntimeStore();
      void store.boot().then(() => {
        if (!disposed && sequence === current) store.setAccountRelayHost(snapshot.selected);
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

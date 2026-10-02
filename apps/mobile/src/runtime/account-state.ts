import { useEffect } from "react";
import { create } from "zustand";
import { getDesktopHost } from "@/desktop/host";
import { getHostRuntimeStore } from "./host-runtime";

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
}

export const useAccountState = create<AccountState>(() => ({
  status: "logged_out",
  center: "",
  name: "",
  hostOnline: false,
  hosts: [],
  stale: true,
  error: null,
  selected: null,
}));

export async function accountCommand(
  command: string,
  args?: Record<string, unknown>,
): Promise<AccountState> {
  const invoke = getDesktopHost()?.invoke;
  if (!invoke) throw new Error("Account relay requires the desktop app.");
  const snapshot = (await invoke(command, args)) as AccountState;
  useAccountState.setState(snapshot);
  return snapshot;
}

/** Mount once next to HostRuntime bootstrap; discovery remains separate from runtime hosts. */
export function AccountRelayLifecycle() {
  useEffect(() => {
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

import type {
  HubAccountConnection,
  HubAccountEvent,
  HubAccountSnapshot,
} from "@ait/client/internal/hub";
import { HubCallError } from "@ait/client/internal/hub";
import { getDesktopHost } from "@/desktop/host";
import { getNativeHubLink } from "./native-account";
import { getBrowserHubLink, supportsBrowserAccount } from "./browser-account";

/** Keep credentials and socket ownership in the desktop main process. */
export async function connectAccountHub(): Promise<{
  connection: HubAccountConnection;
  dispose(): void;
} | null> {
  const desktop = getDesktopHost();
  if (!desktop?.invoke) {
    const connection = await (supportsBrowserAccount() ? getBrowserHubLink() : getNativeHubLink());
    return connection ? { connection, dispose() {} } : null;
  }
  if (!desktop.events?.on) return null;
  let state: HubAccountSnapshot = { state: "stopped", linkId: null, centerUrl: null, methods: [] };
  let sequence = 0;
  let disposed = false;
  const listeners = new Set<() => void>();
  const events = new Set<(event: HubAccountEvent) => void>();
  const cleanup: (() => void)[] = [];
  try {
    cleanup.push(
      await desktop.events.on("account-hub-state", (value) => {
        if (disposed) return;
        ++sequence;
        state = value as HubAccountSnapshot;
        for (const listener of listeners) listener();
      }),
    );
    cleanup.push(
      await desktop.events.on("account-hub-event", (value) => {
        if (!disposed) for (const listener of events) listener(value as HubAccountEvent);
      }),
    );
    const current = sequence;
    const initial = (await desktop.invoke("account_hub_status")) as HubAccountSnapshot;
    if (current === sequence) state = initial;
    return {
      connection: {
        snapshot: () => state,
        subscribe(listener) {
          listeners.add(listener);
          return () => {
            listeners.delete(listener);
          };
        },
        onEvent(listener) {
          events.add(listener);
          return () => {
            events.delete(listener);
          };
        },
        async call<T>(method: string, params: unknown): Promise<T> {
          const value = (await desktop.invoke!("account_hub_call", { method, params })) as {
            result: T;
            error?: { code: string; message: string };
          };
          if (value.error) throw new HubCallError(value.error.code, value.error.message);
          return value.result;
        },
      },
      dispose() {
        disposed = true;
        for (const remove of cleanup) remove();
        listeners.clear();
        events.clear();
      },
    };
  } catch (error) {
    disposed = true;
    for (const remove of cleanup) remove();
    throw error;
  }
}

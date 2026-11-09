import type { StorageValue } from "zustand/middleware";
import type { ValidatedPersistStorage } from "./validated-persist-storage";

/** The revision check runs under the storage lock, before changing either baseline or UI. */
export async function refreshUnchangedPersistedState<State, Revision>(input: {
  name: string;
  storage: ValidatedPersistStorage<State>;
  revision(): Revision;
  flush(): Promise<void>;
  beforeAccept?(): void;
  accept(value: StorageValue<State> | null): boolean;
}): Promise<boolean> {
  const revision = input.revision();
  await input.flush();
  if (revision !== input.revision()) return false;
  return input.storage.refresh(input.name, (value) => {
    input.beforeAccept?.();
    return revision === input.revision() && input.accept(value);
  });
}

/** Shared drafts refresh after another window saves; layouts deliberately stay local. */
export function observePersistedState(input: {
  name: string;
  refresh(): Promise<boolean>;
  delayMs?: number;
}): () => void {
  if (typeof window === "undefined") return () => {};
  let disposed = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const schedule = () => {
    if (disposed) return;
    clearTimeout(timer);
    timer = setTimeout(() => {
      timer = undefined;
      void input
        .refresh()
        .then((accepted) => {
          if (!accepted) schedule();
        })
        .catch((error) => {
          console.warn(`[Storage] Failed to refresh ${input.name}`, error);
        });
    }, input.delayMs ?? 200);
  };
  const changed = (event: StorageEvent) => {
    if (
      (event.key === null || event.key === input.name) &&
      (event.storageArea === null || event.storageArea === window.localStorage)
    )
      schedule();
  };
  window.addEventListener("storage", changed);
  window.addEventListener("focus", schedule);
  return () => {
    disposed = true;
    clearTimeout(timer);
    window.removeEventListener("storage", changed);
    window.removeEventListener("focus", schedule);
  };
}

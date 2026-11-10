import type { PersistStorage } from "zustand/middleware";

export const DRAFT_PERSIST_INTERVAL_MS = 200;
const MAX_DRAFT_PERSIST_RETRIES = 5;
export interface PersistenceScheduler {
  now: () => number;
  schedule: (callback: () => void, delayMs: number) => unknown;
  cancel: (handle: unknown) => void;
}

export interface DraftPersistStorage<T> extends PersistStorage<T> {
  flush: () => Promise<void>;
}

let nextSystemTimerId = 0;
const systemTimers = new Map<number, ReturnType<typeof setTimeout>>();
const systemScheduler: PersistenceScheduler = {
  now: Date.now,
  schedule: (callback, delayMs) => {
    const id = nextSystemTimerId++;
    systemTimers.set(
      id,
      setTimeout(() => {
        systemTimers.delete(id);
        callback();
      }, delayMs),
    );
    return id;
  },
  cancel: (handle) => {
    if (typeof handle !== "number") return;
    const timer = systemTimers.get(handle);
    if (timer === undefined) return;
    clearTimeout(timer);
    systemTimers.delete(handle);
  },
};

export function createDraftPersistStorage<T>(
  storage: PersistStorage<T>,
  scheduler?: PersistenceScheduler,
): DraftPersistStorage<T>;
export function createDraftPersistStorage<T>(
  storage: PersistStorage<T> | undefined,
  scheduler?: PersistenceScheduler,
): DraftPersistStorage<T> | undefined;
export function createDraftPersistStorage<T>(
  storage: PersistStorage<T> | undefined,
  scheduler: PersistenceScheduler = systemScheduler,
): DraftPersistStorage<T> | undefined {
  if (!storage) {
    return undefined;
  }

  let pending: {
    revision: number;
    name: string;
    value: Parameters<typeof storage.setItem>[1];
  } | null = null;
  let timer: unknown = null;
  let lastWriteAt = -Infinity;
  let revision = 0;
  let inFlight: Promise<void> | null = null;
  let failedAttempts = 0;

  const enqueue = (operation: () => Promise<void>): Promise<void> => {
    const result = inFlight ? inFlight.catch(() => undefined).then(operation) : operation();
    inFlight = result;
    const finish = () => {
      if (inFlight === result) inFlight = null;
    };
    void result.then(finish, finish);
    return result;
  };
  const report = (error: unknown) => {
    console.warn("[DraftStore] Failed to persist draft checkpoint", error);
  };

  const cancelTimer = () => {
    if (timer !== null) {
      scheduler.cancel(timer);
      timer = null;
    }
  };
  // Bounded so a persistently failing store cannot spin; the next edit or flush restarts it.
  const scheduleRetry = () => {
    if (failedAttempts >= MAX_DRAFT_PERSIST_RETRIES) return;
    failedAttempts += 1;
    timer ??= scheduler.schedule(
      () => {
        void flush().catch(report);
      },
      DRAFT_PERSIST_INTERVAL_MS * 2 ** failedAttempts,
    );
  };
  const flush = (): Promise<void> => {
    cancelTimer();
    const write = pending;
    pending = null;
    if (!write) {
      return inFlight ?? Promise.resolve();
    }
    lastWriteAt = scheduler.now();
    return enqueue(async () => {
      try {
        await storage.setItem(write.name, write.value);
        failedAttempts = 0;
      } catch (error) {
        if (revision === write.revision) {
          pending = write;
          scheduleRetry();
        }
        throw error;
      }
    });
  };

  return {
    getItem: (name) => storage.getItem(name),
    setItem: (name, value) => {
      pending = { revision: ++revision, name, value };
      failedAttempts = 0;
      const delay = DRAFT_PERSIST_INTERVAL_MS - (scheduler.now() - lastWriteAt);
      if (delay <= 0) {
        return flush().catch(report);
      }
      timer ??= scheduler.schedule(() => {
        void flush().catch(report);
      }, delay);
    },
    removeItem: (name) => {
      cancelTimer();
      pending = null;
      failedAttempts = 0;
      revision += 1;
      lastWriteAt = scheduler.now();
      return enqueue(async () => {
        await storage.removeItem(name);
      });
    },
    flush,
  };
}

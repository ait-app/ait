import { z } from "zod";
import type { PersistStorage, StateStorage, StorageValue } from "zustand/middleware";
import { mergePersistedState, withPersistedStateLock } from "./merge-persisted-state";

export interface ValidatedPersistStorage<State> extends PersistStorage<State> {
  /** Accept a shared snapshot only while the caller's local state is unchanged. */
  refresh(name: string, accept: (value: StorageValue<State> | null) => boolean): Promise<boolean>;
}

export function createValidatedPersistStorage<State>(
  backingStorage: StateStorage,
  stateSchema: z.ZodType<State>,
  options: {
    // Opt in only when callers preserve state references until values change.
    skipUnchanged?: boolean;
    mergeRecordFields?: ReadonlyArray<keyof State>;
    version?: number;
  } = {},
): ValidatedPersistStorage<State> {
  const envelopeSchema = z.strictObject({
    state: stateSchema,
    version: z.number().int().nonnegative().optional(),
  });

  const accepted = new Map<string, StorageValue<State>>();
  const snapshots = new Map<string, State>();
  const coordinated = options.mergeRecordFields !== undefined;
  const run = <T>(name: string, operation: () => Promise<T>): Promise<T> =>
    coordinated ? withPersistedStateLock(name, operation) : operation();

  const read = async (
    name: string,
    version = options.version,
  ): Promise<StorageValue<State> | null> => {
    const raw = await backingStorage.getItem(name);
    if (raw === null) return null;
    let decoded: unknown;
    try {
      decoded = JSON.parse(raw);
    } catch {
      await backingStorage.removeItem(name);
      return null;
    }
    const storedVersion = z
      .object({ version: z.number().int().nonnegative().optional() })
      .safeParse(decoded);
    if (
      version !== undefined &&
      storedVersion.success &&
      (storedVersion.data.version ?? 0) > version
    ) {
      throw new Error(`Persisted state has a newer version: ${name}`);
    }
    const result = envelopeSchema.safeParse(decoded);
    if (!result.success) {
      await backingStorage.removeItem(name);
      return null;
    }
    return result.data;
  };

  return {
    getItem: (name) =>
      run(name, async () => {
        accepted.delete(name);
        const value = await read(name);
        if (value) snapshots.set(name, value.state);
        else snapshots.delete(name);
        return value;
      }),
    setItem: (name, value) =>
      run(name, async () => {
        const previous = accepted.get(name);
        if (
          options.skipUnchanged &&
          previous &&
          previous.state === value.state &&
          previous.version === value.version
        )
          return;
        const result = envelopeSchema.safeParse(value);
        if (!result.success) {
          // A bad local snapshot must not erase another window's valid data.
          if (coordinated) throw new Error(`Invalid persisted state: ${name}`);
          accepted.delete(name);
          await backingStorage.removeItem(name);
          return;
        }
        const saved = coordinated ? await read(name, value.version ?? 0) : null;
        const next =
          saved && saved.version === value.version
            ? {
                ...result.data,
                state: mergePersistedState(
                  snapshots.get(name),
                  result.data.state,
                  saved.state,
                  options.mergeRecordFields ?? [],
                ),
              }
            : result.data;
        if (options.skipUnchanged) accepted.set(name, value);
        try {
          await backingStorage.setItem(name, JSON.stringify(next));
          snapshots.set(name, result.data.state);
        } catch (error) {
          if (accepted.get(name) === value) accepted.delete(name);
          throw error;
        }
      }),
    removeItem: (name) =>
      run(name, async () => {
        accepted.delete(name);
        snapshots.delete(name);
        await backingStorage.removeItem(name);
      }),
    refresh: (name, accept) =>
      run(name, async () => {
        const value = await read(name);
        if (!accept(value)) return false;
        accepted.delete(name);
        if (value) snapshots.set(name, value.state);
        else snapshots.delete(name);
        return true;
      }),
  };
}

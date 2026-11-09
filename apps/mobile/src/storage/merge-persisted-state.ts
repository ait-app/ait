type RecordValue = Record<string, unknown>;

function isRecord(value: unknown): value is RecordValue {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function equal(left: unknown, right: unknown): boolean {
  return left === right || JSON.stringify(left) === JSON.stringify(right);
}

/** Apply this window's changes to the latest saved snapshot, including deletions. */
export function mergePersistedState<State>(
  previous: State | undefined,
  next: State,
  saved: State,
  recordFields: ReadonlyArray<keyof State>,
): State {
  if (!isRecord(next) || !isRecord(saved)) return next;
  const before: RecordValue = isRecord(previous) ? previous : {};
  const merged: RecordValue = { ...saved };
  const fields = new Set<string>(recordFields.map(String));
  for (const field of new Set([...Object.keys(before), ...Object.keys(next)])) {
    if (equal(before[field], next[field])) continue;
    if (!Object.hasOwn(next, field)) {
      delete merged[field];
    } else if (fields.has(field) && isRecord(next[field])) {
      const oldEntries: RecordValue = isRecord(before[field]) ? before[field] : {};
      const entries: RecordValue = { ...(isRecord(saved[field]) ? saved[field] : {}) };
      for (const key of new Set([...Object.keys(oldEntries), ...Object.keys(next[field])])) {
        if (equal(oldEntries[key], next[field][key])) continue;
        if (Object.hasOwn(next[field], key)) {
          Object.defineProperty(entries, key, {
            value: next[field][key],
            enumerable: true,
            writable: true,
            configurable: true,
          });
        } else delete entries[key];
      }
      merged[field] = entries;
    } else {
      merged[field] = next[field];
    }
  }
  return merged as State;
}

const pending = new Map<string, Promise<unknown>>();

/** Desktop's secure app origin shares Web Locks across all of its renderers. */
export function withPersistedStateLock<T>(name: string, operation: () => Promise<T>): Promise<T> {
  if (typeof navigator !== "undefined" && navigator.locks) {
    return navigator.locks.request(`ait:persist:${name}`, operation);
  }
  // Native clients and tests have one JS runtime, so a local queue suffices there.
  const previous = pending.get(name) ?? Promise.resolve();
  const result = previous.catch(() => undefined).then(operation);
  pending.set(name, result);
  const release = () => {
    if (pending.get(name) === result) pending.delete(name);
  };
  void result.then(release, release);
  return result;
}

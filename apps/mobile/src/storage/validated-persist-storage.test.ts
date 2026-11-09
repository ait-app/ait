import { describe, expect, it } from "vitest";
import { z } from "zod";
import type { StateStorage } from "zustand/middleware";
import { createValidatedPersistStorage } from "./validated-persist-storage";

class MemoryStorage implements StateStorage {
  readonly values = new Map<string, string>();

  getItem(name: string): string | null {
    return this.values.get(name) ?? null;
  }

  setItem(name: string, value: string): void {
    this.values.set(name, value);
  }

  removeItem(name: string): void {
    this.values.delete(name);
  }
}

const StateSchema = z.strictObject({ enabled: z.boolean() });

describe("createValidatedPersistStorage", () => {
  it.each([
    ["malformed JSON", "{"],
    ["invalid state", JSON.stringify({ state: { enabled: "yes" }, version: 1 })],
    ["unknown state fields", JSON.stringify({ state: { enabled: true, extra: true }, version: 1 })],
    [
      "unknown envelope fields",
      JSON.stringify({ state: { enabled: true }, version: 1, extra: true }),
    ],
  ])("clears %s", async (_label, stored) => {
    const backing = new MemoryStorage();
    backing.values.set("settings", stored);
    const storage = createValidatedPersistStorage(backing, StateSchema);

    await expect(storage.getItem("settings")).resolves.toBeNull();
    expect(backing.values.has("settings")).toBe(false);
  });

  it("returns schema-validated state", async () => {
    const backing = new MemoryStorage();
    backing.values.set("settings", JSON.stringify({ state: { enabled: true }, version: 1 }));
    const storage = createValidatedPersistStorage(backing, StateSchema);

    await expect(storage.getItem("settings")).resolves.toEqual({
      state: { enabled: true },
      version: 1,
    });
  });

  it("clears a value rejected by the schema before writing", async () => {
    const backing = new MemoryStorage();
    backing.values.set("settings", JSON.stringify({ state: { count: 1 } }));
    const storage = createValidatedPersistStorage(
      backing,
      z.strictObject({ count: z.number().finite() }),
    );

    await storage.setItem("settings", { state: { count: Number.NaN } });

    expect(backing.values.has("settings")).toBe(false);
  });
});

it("skips repeated immutable states only when requested", async () => {
  let writes = 0;
  const backing: StateStorage = {
    getItem: () => null,
    setItem: () => {
      writes += 1;
    },
    removeItem: () => {},
  };
  const value = { state: { enabled: true }, version: 1 };
  const regular = createValidatedPersistStorage(backing, StateSchema);
  await regular.setItem("regular", value);
  await regular.setItem("regular", value);
  expect(writes).toBe(2);

  const deduplicated = createValidatedPersistStorage(backing, StateSchema, { skipUnchanged: true });
  await deduplicated.setItem("first", value);
  await deduplicated.setItem("first", { ...value });
  expect(writes).toBe(3);
  await deduplicated.setItem("second", value);
  expect(writes).toBe(4);
  await deduplicated.setItem("first", { ...value, version: 2 });
  expect(writes).toBe(5);
});

it("retries an unchanged state after a failed save", async () => {
  let attempts = 0;
  const backing = new MemoryStorage();
  const storage = createValidatedPersistStorage(
    {
      getItem: (name) => backing.getItem(name),
      setItem: (name, value) => {
        attempts += 1;
        if (attempts === 1) return Promise.reject(new Error("storage unavailable"));
        backing.setItem(name, value);
      },
      removeItem: (name) => backing.removeItem(name),
    },
    StateSchema,
    { skipUnchanged: true },
  );
  const value = { state: { enabled: true }, version: 1 };
  await expect(storage.setItem("layout", value)).rejects.toThrow("storage unavailable");
  await storage.setItem("layout", value);
  expect(attempts).toBe(2);
  expect(backing.getItem("layout")).toBe(JSON.stringify(value));
});

it("saves the same state again after removal or rehydration clears invalid storage", async () => {
  const backing = new MemoryStorage();
  const storage = createValidatedPersistStorage(backing, StateSchema, { skipUnchanged: true });
  const value = { state: { enabled: true }, version: 1 };
  await storage.setItem("layout", value);
  await storage.removeItem("layout");
  await storage.setItem("layout", value);
  expect(backing.getItem("layout")).toBe(JSON.stringify(value));

  backing.values.set("layout", "{");
  await expect(storage.getItem("layout")).resolves.toBeNull();
  await storage.setItem("layout", value);
  expect(backing.getItem("layout")).toBe(JSON.stringify(value));
});

const SharedStateSchema = z.strictObject({
  records: z.record(z.string(), z.strictObject({ text: z.string() })),
  width: z.number(),
});

function sharedWindows() {
  const backing = new MemoryStorage();
  const createWindow = () =>
    createValidatedPersistStorage(backing, SharedStateSchema, {
      mergeRecordFields: ["records"],
      version: 1,
    });
  const initial = { state: { records: { a: { text: "original" } }, width: 200 }, version: 1 };
  backing.setItem("shared", JSON.stringify(initial));
  return { backing, first: createWindow(), second: createWindow(), initial };
}

describe("shared persistence", () => {
  it("serializes concurrent saves and preserves changes to separate records and fields", async () => {
    const { backing, first, second, initial } = sharedWindows();
    await Promise.all([first.getItem("shared"), second.getItem("shared")]);
    await Promise.all([
      first.setItem("shared", {
        state: { records: { a: { text: "first" } }, width: 300 },
        version: 1,
      }),
      second.setItem("shared", {
        state: { ...initial.state, records: { ...initial.state.records, b: { text: "second" } } },
        version: 1,
      }),
    ]);
    expect(JSON.parse(backing.getItem("shared")!)).toEqual({
      state: { records: { a: { text: "first" }, b: { text: "second" } }, width: 300 },
      version: 1,
    });
  });

  it("does not restore a deleted record when another window saves its unchanged stale copy", async () => {
    const { backing, first, second, initial } = sharedWindows();
    await Promise.all([first.getItem("shared"), second.getItem("shared")]);
    await first.setItem("shared", { state: { records: {}, width: 200 }, version: 1 });
    await second.setItem("shared", { state: { ...initial.state, width: 400 }, version: 1 });
    expect(JSON.parse(backing.getItem("shared")!).state).toEqual({ records: {}, width: 400 });
  });

  it("keeps the local baseline across saves so later edits do not undo unseen remote changes", async () => {
    const { backing, first, second, initial } = sharedWindows();
    await Promise.all([first.getItem("shared"), second.getItem("shared")]);
    await first.setItem("shared", { state: { ...initial.state, width: 300 }, version: 1 });
    const local = { state: { ...initial.state, records: { a: { text: "second" } } }, version: 1 };
    await second.setItem("shared", local);
    await second.setItem("shared", {
      ...local,
      state: { ...local.state, records: { a: { text: "third" } } },
    });
    expect(JSON.parse(backing.getItem("shared")!).state.width).toBe(300);
  });

  it("preserves newer incompatible schemas before attempting to validate or clean up", async () => {
    const { backing, first } = sharedWindows();
    const newer = JSON.stringify({ state: { futureField: true }, version: 2 });
    backing.setItem("shared", newer);
    await expect(first.getItem("shared")).rejects.toThrow("newer version");
    await expect(
      first.setItem("shared", { state: { records: {}, width: 100 }, version: 1 }),
    ).rejects.toThrow("newer version");
    expect(backing.getItem("shared")).toBe(newer);
  });

  it("rejects invalid local saves without deleting shared data", async () => {
    const { backing, first, initial } = sharedWindows();
    await expect(
      first.setItem("shared", { state: { records: {}, width: Number.NaN }, version: 1 }),
    ).rejects.toThrow("Invalid persisted state");
    expect(JSON.parse(backing.getItem("shared")!)).toEqual(initial);
  });

  it("leaves the baseline unchanged when a refresh is declined due to a local edit", async () => {
    const { backing, first, second, initial } = sharedWindows();
    await Promise.all([first.getItem("shared"), second.getItem("shared")]);
    await first.setItem("shared", {
      state: { records: { a: { text: "first" } }, width: 200 },
      version: 1,
    });
    await expect(second.refresh("shared", () => false)).resolves.toBe(false);
    await second.setItem("shared", { state: { ...initial.state, width: 400 }, version: 1 });
    expect(JSON.parse(backing.getItem("shared")!).state).toEqual({
      records: { a: { text: "first" } },
      width: 400,
    });
  });

  it("rebases the local baseline when a shared refresh is accepted", async () => {
    const { backing, first, second } = sharedWindows();
    await Promise.all([first.getItem("shared"), second.getItem("shared")]);
    const refreshed = { state: { records: { a: { text: "first" } }, width: 200 }, version: 1 };
    await first.setItem("shared", refreshed);
    await second.refresh("shared", () => true);
    await first.setItem("shared", {
      state: { ...refreshed.state, records: { a: { text: "newer" } } },
      version: 1,
    });
    await second.setItem("shared", { state: { ...refreshed.state, width: 400 }, version: 1 });
    expect(JSON.parse(backing.getItem("shared")!).state).toEqual({
      records: { a: { text: "newer" } },
      width: 400,
    });
  });
});

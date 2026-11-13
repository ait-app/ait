// @vitest-environment jsdom
import { expect, it, vi } from "vitest";
import { createBtwVisibilityStore } from "./btw-visibility-store";

function memoryStorage() {
  const data = new Map<string, string>();
  return {
    data,
    storage: {
      getItem: (key: string) => data.get(key) ?? null,
      setItem: (key: string, value: string) => {
        data.set(key, value);
      },
      removeItem: (key: string) => {
        data.delete(key);
      },
    },
  };
}

it("retains ended host/session identities across reload without storing conversation content", async () => {
  const { data, storage } = memoryStorage();
  const first = createBtwVisibilityStore(storage);
  await first.persist.rehydrate();
  first.getState().end("host:session", "request-one");
  first.getState().end("other-host:session", "other-request");
  await vi.waitFor(() => expect(data.has("btw-visibility")).toBe(true));
  const restored = createBtwVisibilityStore(storage);
  await restored.persist.rehydrate();
  expect(restored.getState().hydrated).toBe(true);
  expect(restored.getState().ended).toEqual([
    ["host:session", "request-one"],
    ["other-host:session", "other-request"],
  ]);
  restored.getState().end("host:session", "request-two");
  expect(restored.getState().ended).toEqual([
    ["other-host:session", "other-request"],
    ["host:session", "request-two"],
  ]);
  await vi.waitFor(() =>
    expect(JSON.parse(data.get("btw-visibility")!).state).toEqual({
      ended: restored.getState().ended,
    }),
  );
});

it("keeps at most 256 ended identities, dropping the oldest first", async () => {
  const { storage } = memoryStorage();
  const store = createBtwVisibilityStore(storage);
  await store.persist.rehydrate();
  for (let index = 0; index < 300; index += 1) {
    store.getState().end(`host:${index}`, `request-${index}`);
  }
  const ended = store.getState().ended;
  expect(ended).toHaveLength(256);
  expect(ended[0]).toEqual(["host:44", "request-44"]);
  expect(ended.at(-1)).toEqual(["host:299", "request-299"]);
});

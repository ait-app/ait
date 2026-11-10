// @vitest-environment jsdom
import { expect, it, vi } from "vitest";
import { createBtwVisibilityStore } from "./btw-visibility-store";

it("retains ended request identities across reload without storing conversation content", async () => {
  const data = new Map<string, string>();
  const storage = {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => {
      data.set(key, value);
    },
    removeItem: (key: string) => {
      data.delete(key);
    },
  };
  const first = createBtwVisibilityStore(storage);
  await first.persist.rehydrate();
  first.getState().end("host:agent", "request-one");
  first.getState().end("other-host:agent", "other-request");
  await vi.waitFor(() => expect(data.has("btw-visibility")).toBe(true));
  const restored = createBtwVisibilityStore(storage);
  await restored.persist.rehydrate();
  expect(restored.getState().hydrated).toBe(true);
  expect(restored.getState().ended).toEqual([
    ["host:agent", "request-one"],
    ["other-host:agent", "other-request"],
  ]);
  restored.getState().end("host:agent", "request-two");
  expect(restored.getState().ended).toEqual([
    ["other-host:agent", "other-request"],
    ["host:agent", "request-two"],
  ]);
  await vi.waitFor(() =>
    expect(JSON.parse(data.get("btw-visibility")!).state).toEqual({
      ended: restored.getState().ended,
    }),
  );
});

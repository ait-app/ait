/** @vitest-environment jsdom */
import { afterEach, expect, it, vi } from "vitest";
import { z } from "zod";
import { createValidatedPersistStorage } from "./validated-persist-storage";
import { observePersistedState, refreshUnchangedPersistedState } from "./sync-persisted-state";

afterEach(() => {
  vi.useRealTimers();
});

it("does not replace input edited while a draft checkpoint is flushing", async () => {
  let revision = 0;
  const accept = vi.fn(() => true);
  const storage = createValidatedPersistStorage(
    { getItem: () => null, setItem: () => {}, removeItem: () => {} },
    z.string(),
    { mergeRecordFields: [] },
  );
  const result = await refreshUnchangedPersistedState({
    name: "drafts",
    storage,
    revision: () => revision,
    flush: async () => {
      revision += 1;
    },
    accept,
  });
  expect(result).toBe(false);
  expect(accept).not.toHaveBeenCalled();
});

it("checks for edits again after the asynchronous read, inside the storage lock", async () => {
  let revision = 0;
  const accept = vi.fn(() => true);
  const storage = createValidatedPersistStorage(
    {
      getItem: async () => {
        revision += 1;
        return null;
      },
      setItem: () => {},
      removeItem: () => {},
    },
    z.string(),
    { mergeRecordFields: [] },
  );
  expect(
    await refreshUnchangedPersistedState({
      name: "drafts",
      storage,
      revision: () => revision,
      flush: async () => {},
      accept,
    }),
  ).toBe(false);
  expect(accept).not.toHaveBeenCalled();
});

it("keeps unsaved input when checkpoint persistence fails", async () => {
  const accept = vi.fn(() => true);
  const storage = createValidatedPersistStorage(
    { getItem: () => null, setItem: () => {}, removeItem: () => {} },
    z.string(),
  );
  await expect(
    refreshUnchangedPersistedState({
      name: "drafts",
      storage,
      revision: () => 0,
      flush: async () => {
        throw new Error("disk full");
      },
      accept,
    }),
  ).rejects.toThrow("disk full");
  expect(accept).not.toHaveBeenCalled();
});

it("refreshes shared drafts on matching storage changes and focus, and removes listeners", async () => {
  vi.useFakeTimers();
  const refresh = vi.fn(async () => true);
  const stop = observePersistedState({ name: "drafts", refresh });
  try {
    window.dispatchEvent(new StorageEvent("storage", { key: "layout" }));
    await vi.advanceTimersByTimeAsync(200);
    expect(refresh).not.toHaveBeenCalled();
    window.dispatchEvent(new StorageEvent("storage", { key: "drafts" }));
    window.dispatchEvent(new StorageEvent("storage", { key: "drafts" }));
    await vi.advanceTimersByTimeAsync(200);
    expect(refresh).toHaveBeenCalledTimes(1);
    window.dispatchEvent(new Event("focus"));
    await vi.advanceTimersByTimeAsync(200);
    expect(refresh).toHaveBeenCalledTimes(2);
    stop();
    window.dispatchEvent(new StorageEvent("storage", { key: "drafts" }));
    await vi.advanceTimersByTimeAsync(200);
    expect(refresh).toHaveBeenCalledTimes(2);
  } finally {
    stop();
  }
});

it("retries a refresh skipped during editing after the checkpoint interval", async () => {
  vi.useFakeTimers();
  const refresh = vi.fn().mockResolvedValueOnce(false).mockResolvedValue(true);
  const stop = observePersistedState({ name: "drafts", refresh });
  try {
    window.dispatchEvent(new StorageEvent("storage", { key: "drafts" }));
    await vi.advanceTimersByTimeAsync(400);
    expect(refresh).toHaveBeenCalledTimes(2);
  } finally {
    stop();
  }
});

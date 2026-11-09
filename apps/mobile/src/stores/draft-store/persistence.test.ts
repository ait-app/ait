import { describe, expect, it, vi } from "vitest";
import type { PersistStorage, StorageValue } from "zustand/middleware";
import {
  createDraftPersistStorage,
  DRAFT_PERSIST_INTERVAL_MS,
  type PersistenceScheduler,
} from "./persistence";

interface DraftState {
  text: string;
}

function createDraftPersistence() {
  let nowMs = 0;
  let saved: StorageValue<DraftState> | null = null;
  let scheduled: { callback: () => void; dueAt: number } | null = null;
  const storage: PersistStorage<DraftState> = {
    getItem: () => saved,
    setItem: (_name, value) => {
      saved = value;
    },
    removeItem: () => {
      saved = null;
    },
  };
  const scheduler: PersistenceScheduler = {
    now: () => nowMs,
    schedule: (callback, delayMs) => (scheduled = { callback, dueAt: nowMs + delayMs }),
    cancel: () => {
      scheduled = null;
    },
  };
  const drafts = createDraftPersistStorage(storage, scheduler);

  return {
    save(text: string) {
      drafts.setItem("drafts", { state: { text } });
    },
    remove() {
      return drafts.removeItem("drafts");
    },
    flush() {
      return drafts.flush();
    },
    advance(ms: number) {
      nowMs += ms;
      if (scheduled && scheduled.dueAt <= nowMs) {
        const { callback } = scheduled;
        scheduled = null;
        callback();
      }
    },
    text() {
      return saved?.state.text ?? null;
    },
  };
}

describe("draft persistence", () => {
  it("checkpoints the first change and the latest change in each interval", async () => {
    const drafts = createDraftPersistence();

    drafts.save("a");
    drafts.save("ab");
    drafts.save("abc");
    expect(drafts.text()).toBe("a");

    drafts.advance(DRAFT_PERSIST_INTERVAL_MS - 1);
    expect(drafts.text()).toBe("a");

    drafts.advance(1);
    await drafts.flush();
    expect(drafts.text()).toBe("abc");
  });

  it("does not restore a pending draft after storage is cleared", async () => {
    const drafts = createDraftPersistence();

    drafts.save("first checkpoint");
    drafts.save("pending checkpoint");
    await drafts.remove();
    drafts.advance(DRAFT_PERSIST_INTERVAL_MS);

    expect(drafts.text()).toBeNull();
  });

  it("continues checkpointing the latest change across consecutive intervals", async () => {
    const drafts = createDraftPersistence();

    drafts.save("first");
    drafts.save("first interval");
    drafts.advance(DRAFT_PERSIST_INTERVAL_MS);
    await drafts.flush();
    expect(drafts.text()).toBe("first interval");

    drafts.save("second");
    drafts.save("second interval");
    drafts.advance(DRAFT_PERSIST_INTERVAL_MS);
    await drafts.flush();
    expect(drafts.text()).toBe("second interval");
  });

  it("flushes the latest pending change before the interval ends", async () => {
    const drafts = createDraftPersistence();

    drafts.save("first checkpoint");
    drafts.save("pending checkpoint");
    await drafts.flush();

    expect(drafts.text()).toBe("pending checkpoint");
  });

  it("waits for a checkpoint already in flight before reporting a successful flush", async () => {
    let finish!: () => void;
    const drafts = createDraftPersistStorage<DraftState>({
      getItem: () => null,
      removeItem: () => {},
      setItem: () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    });
    drafts.setItem("drafts", { state: { text: "pending" } });
    const settled = vi.fn();
    const flush = drafts.flush().then(settled);
    await Promise.resolve();
    expect(settled).not.toHaveBeenCalled();
    finish();
    await flush;
    expect(settled).toHaveBeenCalledTimes(1);
  });

  it("retains the latest checkpoint for retry after a failed write", async () => {
    const warning = vi.spyOn(console, "warn").mockImplementation(() => {});
    let attempts = 0;
    let saved = "";
    const drafts = createDraftPersistStorage<DraftState>({
      getItem: () => null,
      removeItem: () => {},
      setItem: (_name, value) => {
        if (++attempts <= 2) throw new Error("disk full");
        saved = value.state.text;
      },
    });
    try {
      await drafts.setItem("drafts", { state: { text: "unsaved" } });
      await expect(drafts.flush()).rejects.toThrow("disk full");
      await drafts.flush();
      expect(saved).toBe("unsaved");
    } finally {
      warning.mockRestore();
    }
  });

  it("serializes a clear after queued saves so a delayed save cannot restore sent input", async () => {
    let finish!: () => void;
    let saved: string | null = null;
    const drafts = createDraftPersistStorage<DraftState>({
      getItem: () => null,
      removeItem: () => {
        saved = null;
      },
      setItem: async (_name, value) => {
        await new Promise<void>((resolve) => {
          finish = resolve;
        });
        saved = value.state.text;
      },
    });
    drafts.setItem("drafts", { state: { text: "before sending" } });
    const clear = drafts.removeItem("drafts");
    finish();
    await clear;
    await drafts.flush();
    expect(saved).toBeNull();
  });
});

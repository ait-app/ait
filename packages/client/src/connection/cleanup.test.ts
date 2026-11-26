import { afterEach, expect, test, vi } from "vitest";
import { SubscriptionCleanup } from "./cleanup.js";

afterEach(() => vi.useRealTimers());

function fixture(
  release = vi.fn(async (_id: string, _timeout: number): Promise<unknown> => undefined),
) {
  const expired = vi.fn();
  const completed = vi.fn();
  const cleanup = new SubscriptionCleanup({
    release,
    expired,
    completed,
    retryable: (error) => error === "busy",
  });
  return { cleanup, release, expired, completed };
}

test("deduplicates cleanup and retries resource rejection with its own backoff", async () => {
  vi.useFakeTimers();
  vi.spyOn(Math, "random").mockReturnValue(0.5);
  const f = fixture(vi.fn().mockRejectedValueOnce("busy").mockResolvedValue(undefined));
  const first = f.cleanup.release("id");
  expect(f.cleanup.release("id")).toBe(first);
  await vi.advanceTimersByTimeAsync(249);
  expect(f.release).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(1);
  await first;
  expect(f.release).toHaveBeenCalledTimes(2);
  expect(f.expired).not.toHaveBeenCalled();
  expect(f.completed).toHaveBeenCalledWith(250, 2);
  expect(vi.getTimerCount()).toBe(0);
  vi.restoreAllMocks();
});

test("a stuck generation reconnects exactly once at its total deadline", async () => {
  vi.useFakeTimers();
  const f = fixture(vi.fn(() => new Promise(() => {})));
  const pending = [f.cleanup.release("a"), f.cleanup.release("b")];
  await vi.advanceTimersByTimeAsync(59_999);
  expect(f.expired).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(1);
  await Promise.all(pending);
  expect(f.expired).toHaveBeenCalledOnce();
  expect(vi.getTimerCount()).toBe(0);
});

test("old retry completions cannot release a new connection's identical ID", async () => {
  vi.useFakeTimers();
  let reject!: (error: unknown) => void;
  const f = fixture(
    vi
      .fn()
      .mockImplementationOnce(
        () =>
          new Promise((_, fail) => {
            reject = fail;
          }),
      )
      .mockResolvedValue(undefined),
  );
  const old = f.cleanup.release("same-id");
  f.cleanup.reset();
  await f.cleanup.release("same-id");
  reject("busy");
  await old;
  await vi.advanceTimersByTimeAsync(60_000);
  expect(f.release).toHaveBeenCalledTimes(2);
  expect(f.expired).not.toHaveBeenCalled();
});

test("a definite refusal is surfaced without forcing a transport reconnect", async () => {
  const error = new Error("invalid ID");
  const f = fixture(vi.fn().mockRejectedValue(error));
  await expect(f.cleanup.release("id")).rejects.toBe(error);
  expect(f.expired).not.toHaveBeenCalled();
});

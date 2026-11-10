/**
 * @vitest-environment jsdom
 */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { useTrackedRefresh } from "./use-tracked-refresh";

function deferred() {
  let resolve!: () => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<void>((nextResolve, nextReject) => {
    resolve = nextResolve;
    reject = nextReject;
  });
  return { promise, resolve, reject };
}

afterEach(cleanup);

describe("useTrackedRefresh", () => {
  it("shows the indicator until the refresh settles", async () => {
    const pending = deferred();
    const { result } = renderHook(() => useTrackedRefresh(() => pending.promise));

    expect(result.current.isRefreshing).toBe(false);
    act(() => result.current.onRefresh());
    expect(result.current.isRefreshing).toBe(true);

    await act(async () => pending.resolve());
    expect(result.current.isRefreshing).toBe(false);
  });

  it("clears the indicator when the refresh fails", async () => {
    const pending = deferred();
    const { result } = renderHook(() => useTrackedRefresh(() => pending.promise));

    act(() => result.current.onRefresh());
    await act(async () => pending.reject(new Error("offline")));
    expect(result.current.isRefreshing).toBe(false);
  });

  it("keeps the indicator for a newer refresh when an older one settles", async () => {
    const first = deferred();
    const second = deferred();
    const refreshes = [first, second];
    const { result } = renderHook(() =>
      useTrackedRefresh(() => refreshes.shift()?.promise ?? Promise.resolve()),
    );

    act(() => result.current.onRefresh());
    act(() => result.current.onRefresh());
    await act(async () => first.resolve());
    expect(result.current.isRefreshing).toBe(true);

    await act(async () => second.resolve());
    expect(result.current.isRefreshing).toBe(false);
  });
});

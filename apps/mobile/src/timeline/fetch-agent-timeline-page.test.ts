import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { expect, test, vi } from "vitest";
import { fetchAgentTimelinePage } from "./fetch-agent-timeline-page";

type Page = Awaited<ReturnType<DaemonClient["fetchAgentTimeline"]>>;
const cached = { epoch: "stored", historyNeedsRefresh: true } as Page;
const fresh = { epoch: "fresh", historyNeedsRefresh: false } as Page;
const request = { direction: "tail", limit: 40, projection: "projected" } as const;

function fixture() {
  let finish: (page: Page) => void = () => {};
  let fail: (error: Error) => void = () => {};
  const refresh = new Promise<Page>((resolve, reject) => {
    finish = resolve;
    fail = reject;
  });
  const ports = {
    scope: {},
    supportsCachedReads: true,
    fetch: vi.fn(async (options: Parameters<DaemonClient["fetchAgentTimeline"]>[1]) =>
      options?.allowCached ? cached : refresh,
    ),
    apply: vi.fn(),
    isCurrent: vi.fn(() => true),
    reportError: vi.fn(),
  };
  return { ports, finish, fail };
}

test("shows the tail before native revalidation completes and then applies the fresh tail", async () => {
  const { ports, finish } = fixture();

  await expect(fetchAgentTimelinePage("agent", request, ports)).resolves.toBe(cached);
  expect(ports.apply).toHaveBeenCalledExactlyOnceWith(cached);
  expect(ports.fetch.mock.calls).toEqual([
    [{ ...request, allowCached: true }],
    [{ ...request, allowCached: false }],
  ]);

  finish(fresh);
  await vi.waitFor(() => expect(ports.apply).toHaveBeenLastCalledWith(fresh));
});

test("shares one native revalidation for concurrent cached pages of the same agent", async () => {
  const { ports, finish } = fixture();
  await Promise.all([
    fetchAgentTimelinePage("agent", request, ports),
    fetchAgentTimelinePage("agent", { ...request, direction: "before" }, ports),
  ]);
  expect(ports.fetch.mock.calls.filter(([options]) => options?.allowCached === false)).toHaveLength(
    1,
  );
  finish(fresh);
  await vi.waitFor(() => expect(ports.apply).toHaveBeenCalledWith(fresh));
});

test("drops late revalidation from a replaced owner or connection", async () => {
  const { ports, finish } = fixture();
  await fetchAgentTimelinePage("agent", request, ports);
  ports.isCurrent.mockReturnValue(false);
  finish(fresh);
  await vi.waitFor(() => expect(ports.isCurrent).toHaveBeenCalledTimes(2));
  expect(ports.apply).toHaveBeenCalledExactlyOnceWith(cached);
});

test("retains the displayed tail when native revalidation fails and allows a later retry", async () => {
  const { ports, fail } = fixture();
  await fetchAgentTimelinePage("agent", request, ports);
  const error = new Error("native unavailable");
  fail(error);
  await vi.waitFor(() => expect(ports.reportError).toHaveBeenCalledExactlyOnceWith(error));
  expect(ports.apply).toHaveBeenCalledExactlyOnceWith(cached);

  ports.fetch.mockImplementation(async (options) => (options?.allowCached ? cached : fresh));
  await fetchAgentTimelinePage("agent", request, ports);
  await vi.waitFor(() => expect(ports.apply).toHaveBeenLastCalledWith(fresh));
  expect(ports.fetch.mock.calls.filter(([options]) => options?.allowCached === false)).toHaveLength(
    2,
  );
});

test.each(["old host", "explicit fresh"])("keeps %s reads strict", async (kind) => {
  const { ports } = fixture();
  ports.supportsCachedReads = kind !== "old host";
  ports.fetch.mockResolvedValue(fresh);

  await fetchAgentTimelinePage("agent", { ...request, allowCached: kind === "old host" }, ports);

  expect(ports.fetch).toHaveBeenCalledExactlyOnceWith(request);
  expect(ports.apply).toHaveBeenCalledExactlyOnceWith(fresh);
});

test("does not revalidate a history already reconciled by the daemon", async () => {
  const { ports } = fixture();
  ports.fetch.mockResolvedValue(fresh);
  await fetchAgentTimelinePage("agent", request, ports);
  expect(ports.fetch).toHaveBeenCalledTimes(1);
});

test("a failed initial page still rejects without displaying or revalidating it", async () => {
  const { ports } = fixture();
  ports.fetch.mockRejectedValue(new Error("storage unavailable"));
  await expect(fetchAgentTimelinePage("agent", request, ports)).rejects.toThrow(
    "storage unavailable",
  );
  expect(ports.apply).not.toHaveBeenCalled();
  expect(ports.fetch).toHaveBeenCalledTimes(1);
});

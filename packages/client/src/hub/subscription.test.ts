import { afterEach, describe, expect, it, vi } from "vitest";
import { observeHubEvents } from "./subscription.js";
import { HubCallError } from "./calls.js";
import type { HubAccountConnection, HubAccountEvent, HubAccountSnapshot } from "./account.js";

const event = (cursor: number): HubAccountEvent => ({
  subscriptionId: "sub-1",
  cursor,
  hostId: "host",
  eventId: `event-${cursor}`,
  method: "agent.update",
  params: {},
});
function fixture() {
  vi.useFakeTimers();
  let state: HubAccountSnapshot = {
    state: "online",
    linkId: "link-1",
    centerUrl: "https://center",
    methods: ["session.events.set_subscription.request"],
  };
  const listeners = new Set<() => void>();
  let receive = (_event: HubAccountEvent) => {};
  const call = vi.fn(async (method: string, params: unknown): Promise<unknown> => {
    if (method === "hub.events.get_cursor") return { cursor: 10 };
    if (method === "subscription.release.request") return {};
    return {
      subscriptionId: (params as { subscriptionId: string }).subscriptionId,
      events: [],
      cursor: 10,
      hasMore: false,
      active: true,
    };
  });
  const connection: HubAccountConnection = {
    snapshot: () => state,
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    onEvent(listener) {
      receive = listener;
      return () => {
        receive = () => {};
      };
    },
    call: <T>(method: string, params: unknown) => call(method, params) as Promise<T>,
  };
  let id = 0;
  return {
    connection,
    call,
    receive: (value: HubAccountEvent) => receive(value),
    flush: () => vi.advanceTimersByTimeAsync(0),
    change(value: Partial<HubAccountSnapshot>) {
      state = { ...state, ...value };
      for (const listener of listeners) listener();
    },
    options: {
      events: ["agent.update"],
      randomUUID: () => `sub-${++id}`,
      snapshot: vi.fn(async () => {}),
      event: vi.fn(async (_event: HubAccountEvent) => {}),
    },
  };
}
afterEach(() => vi.useRealTimers());
describe("account subscriptions", () => {
  it("captures a bootstrap cursor before loading snapshots and pages until active", async () => {
    const h = fixture();
    h.call.mockImplementation(async (method, params) => {
      if (method === "hub.events.get_cursor") return { cursor: 10 };
      const since = (params as { since: number }).since;
      return {
        subscriptionId: "sub-1",
        events: [event(since + 1)],
        cursor: since + 1,
        hasMore: since === 10,
        active: since !== 10,
      };
    });
    const stop = observeHubEvents(h.connection, h.options);
    await h.flush();
    expect(h.options.snapshot).toHaveBeenCalledOnce();
    expect(h.call.mock.calls[0][0]).toBe("hub.events.get_cursor");
    expect(
      h.call.mock.calls.slice(1).map(([, params]) => (params as { since: number }).since),
    ).toEqual([10, 11]);
    expect(h.options.event.mock.calls.map(([value]) => value.cursor)).toEqual([11, 12]);
    stop();
  });
  it("keeps physical-resume interests and reattaches fresh links from the consumed cursor", async () => {
    const h = fixture();
    const stop = observeHubEvents(h.connection, h.options);
    await h.flush();
    h.receive(event(11));
    await h.flush();
    h.change({ state: "reconnecting" });
    h.change({ state: "online" });
    await h.flush();
    expect(
      h.call.mock.calls.filter(([method]) => method.includes("set_subscription")),
    ).toHaveLength(1);
    h.change({ linkId: "link-2" });
    await h.flush();
    expect(h.call).toHaveBeenLastCalledWith(
      "session.events.set_subscription.request",
      expect.objectContaining({ subscriptionId: "sub-2", since: 11 }),
    );
    expect(h.options.snapshot).toHaveBeenCalledTimes(2);
    stop();
  });
  it("does not advance past a failed event handler", async () => {
    const h = fixture();
    const stop = observeHubEvents(h.connection, h.options);
    await h.flush();
    h.options.event.mockRejectedValueOnce(new Error("snapshot unavailable"));
    h.receive(event(11));
    await h.flush();
    await vi.advanceTimersByTimeAsync(3000);
    expect(h.call).toHaveBeenLastCalledWith(
      "session.events.set_subscription.request",
      expect.objectContaining({ since: 10 }),
    );
    stop();
  });
  it("loads a new baseline when the replay window expires and releases on disposal", async () => {
    const h = fixture();
    const stop = observeHubEvents(h.connection, h.options);
    await h.flush();
    h.call.mockRejectedValueOnce(new HubCallError("replay_expired", "expired"));
    h.change({ linkId: "link-2" });
    await h.flush();
    await vi.advanceTimersByTimeAsync(3000);
    expect(h.options.snapshot).toHaveBeenCalledTimes(3);
    stop();
    expect(h.call).toHaveBeenLastCalledWith("subscription.release.request", {
      subscriptionId: "sub-2",
    });
    h.receive({ ...event(99), subscriptionId: "sub-2" });
    await h.flush();
    expect(h.options.event).not.toHaveBeenCalled();
  });
});

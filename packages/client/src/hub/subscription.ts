import { HubCallError } from "./calls.js";
import type { HubAccountConnection, HubAccountEvent } from "./account.js";

interface SubscriptionPage {
  subscriptionId: string;
  events: Omit<HubAccountEvent, "subscriptionId">[];
  cursor: number;
  hasMore: boolean;
  active: boolean;
}

/**
 * Own one account interest. Physical resumes retain it; fresh links replay from the last
 * successfully consumed cursor. Bootstrap captures the cursor before loading the snapshot.
 */
export function observeHubEvents(
  connection: HubAccountConnection,
  options: {
    events: string[];
    hostIds?: string[];
    resourceIds?: string[];
    snapshot(): Promise<void>;
    event(event: HubAccountEvent): Promise<void> | void;
    error?(error: unknown): void;
    active?(value: boolean): void;
    randomUUID?: () => string;
  },
): () => void {
  let stopped = false;
  let generation = 0;
  let link: string | null = null;
  let id = "";
  let cursor: number | null = null;
  let running = false;
  let active = false;
  let needsSnapshot = true;
  let queue: HubAccountEvent[] = [];
  let timer: ReturnType<typeof setTimeout> | undefined;
  const uuid = options.randomUUID ?? (() => globalThis.crypto.randomUUID());
  const release = (subscriptionId: string) => {
    if (subscriptionId && connection.snapshot().state === "online")
      void connection.call("subscription.release.request", { subscriptionId }).catch(() => {});
  };
  const consume = async (event: HubAccountEvent, epoch: number) => {
    if (stopped || epoch !== generation || event.cursor <= (cursor ?? -1)) return;
    await options.event(event);
    if (!stopped && epoch === generation) cursor = event.cursor;
  };
  const run = async () => {
    if (running || stopped || connection.snapshot().state !== "online") return;
    running = true;
    const epoch = generation;
    try {
      if (cursor === null) {
        const value = await connection.call<{ cursor: number }>("hub.events.get_cursor", {});
        if (stopped || epoch !== generation) return;
        cursor = value.cursor;
      }
      if (needsSnapshot) {
        await options.snapshot();
        if (stopped || epoch !== generation) return;
        needsSnapshot = false;
      }
      while (!active && !stopped && epoch === generation) {
        const page: SubscriptionPage = await connection.call<SubscriptionPage>(
          "session.events.set_subscription.request",
          {
            subscriptionId: id,
            events: options.events,
            hostIds: options.hostIds ?? [],
            resourceIds: options.resourceIds ?? [],
            since: cursor,
            limit: 100,
          },
        );
        if (stopped || epoch !== generation) return;
        for (const event of page.events) await consume({ ...event, subscriptionId: id }, epoch);
        if (stopped || epoch !== generation) return;
        cursor = page.cursor;
        active = page.active;
        options.active?.(active);
        if (!active && !page.hasMore) throw new Error("Invalid Hub subscription page.");
      }
      while (queue.length && !stopped && epoch === generation) await consume(queue.shift()!, epoch);
    } catch (error) {
      if (stopped || epoch !== generation) return;
      options.error?.(error);
      if (
        error instanceof HubCallError &&
        (error.code === "replay_expired" || error.code === "bad_request")
      ) {
        cursor = null;
        needsSnapshot = true;
      }
      // A call may have committed before timing out. Retrying the same ID replaces its interest.
      active = false;
      options.active?.(false);
      needsSnapshot = true;
      queue = [];
      timer = setTimeout(() => void run(), 3000);
    } finally {
      running = false;
      if (!stopped && epoch !== generation) void run();
    }
  };
  const sync = () => {
    const state = connection.snapshot();
    if (state.linkId !== link) {
      ++generation;
      link = state.linkId;
      id = uuid();
      active = false;
      needsSnapshot = true;
      options.active?.(false);
      queue = [];
      clearTimeout(timer);
    }
    if (state.methods.includes("session.events.set_subscription.request")) void run();
  };
  const removeEvent = connection.onEvent((event) => {
    if (stopped || event.subscriptionId !== id) return;
    if (queue.length >= 256) {
      // Release this slow interest; replay from the consumed cursor without an unbounded queue.
      release(id);
      ++generation;
      id = uuid();
      queue = [];
      active = false;
      options.active?.(false);
      void run();
      return;
    }
    queue.push(event);
    if (active) void run();
  });
  const removeState = connection.subscribe(sync);
  sync();
  return () => {
    stopped = true;
    options.active?.(false);
    ++generation;
    clearTimeout(timer);
    removeState();
    removeEvent();
    release(id);
    queue = [];
  };
}

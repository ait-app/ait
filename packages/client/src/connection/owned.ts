import type { SessionOutboundMessage } from "@ait/protocol/messages";

export type SubscriptionSnapshot<T> = T & { subscriptionId: string };

export interface SubscriptionObserver<T> {
  snapshot(snapshot: T): void;
  update(message: SessionOutboundMessage): void;
  error?(error: unknown): void;
}

/** A stable local lifetime. IDs change on reconnect; legacy IDs identify local listeners only. */
export interface OwnedSubscription<T> {
  readonly subscriptionId: string | null;
  readonly ready: Promise<SubscriptionSnapshot<T>>;
  subscribe(observer: SubscriptionObserver<SubscriptionSnapshot<T>>): () => void;
  release(): Promise<void>;
}

interface Registration {
  priority: number;
  start(): Promise<void> | null;
  disconnected(): void;
  receive(message: SessionOutboundMessage): void;
  release(): Promise<void>;
}

export interface OwnedSubscriptionsHost {
  release(subscriptionId: string): Promise<void>;
  failed(error: unknown): void;
  restoring?(active: boolean): void;
}

const MAX_BOOTSTRAP_UPDATES = 128;

/** Transport routing and lifetime only; query semantics remain with each domain. */
export class OwnedSubscriptions {
  private readonly registrations = new Set<Registration>();
  private readonly routes = new Map<string, Registration>();
  private generation = 0;
  private connected = false;
  private readonly queued = new Set<Registration>();
  private readonly active = new Map<Registration, Promise<void>>();
  private readonly idle = new Set<() => void>();
  private restoring = false;

  constructor(private readonly host: OwnedSubscriptionsHost) {}

  observe<T>(
    request: (accept: (snapshot: T) => void) => Promise<T>,
    signal?: AbortSignal,
    requestFailed?: () => void,
    priority = 2,
  ): OwnedSubscription<T> {
    let id: string | null = null;
    let snapshot: SubscriptionSnapshot<T> | undefined;
    let pending: Promise<void> | null = null;
    let attempt = 0;
    let teardown: Promise<void> | null = null;
    let disposed = false;
    let overflow = false;
    const buffered: SessionOutboundMessage[] = [];
    const observers = new Set<SubscriptionObserver<SubscriptionSnapshot<T>>>();
    let resolve!: (snapshot: SubscriptionSnapshot<T>) => void;
    let reject!: (error: unknown) => void;
    const ready = new Promise<SubscriptionSnapshot<T>>((accept, fail) => {
      resolve = accept;
      reject = fail;
    });
    void ready.catch(() => {});

    const deliver = (
      observer: SubscriptionObserver<SubscriptionSnapshot<T>>,
      message?: SessionOutboundMessage,
    ) => {
      try {
        if (message) observer.update(message);
        else if (snapshot) observer.snapshot(snapshot);
      } catch (error) {
        this.host.failed(error);
      }
    };
    const releaseId = async () => {
      const released = id;
      id = null;
      if (!released) return;
      this.routes.delete(released);
      await this.host.release(released);
    };
    const registration: Registration = {
      priority,
      start: () => {
        if (disposed || pending || id || !this.connected) return null;
        const generation = this.generation;
        const currentAttempt = ++attempt;
        pending = request((value) => {
          if (generation !== this.generation) return;
          assertSubscriptionSnapshot(value);
          id = value.subscriptionId;
          snapshot = value;
          buffered.length = 0;
          overflow = false;
          if (disposed) return;
          this.routes.set(id, registration);
          for (const observer of observers) deliver(observer);
        })
          .then(() => {
            if (generation === this.generation && !disposed) {
              if (!snapshot) throw new Error("The host omitted its subscription snapshot");
              resolve(snapshot);
            }
            return undefined;
          })
          .catch((error) => {
            if (generation !== this.generation) return;
            reject(error);
            for (const observer of observers) {
              try {
                observer.error?.(error);
              } catch (failure) {
                this.host.failed(failure);
              }
            }
            void handle.release().catch(this.host.failed);
            this.host.failed(error);
            requestFailed?.();
            return undefined;
          })
          .finally(() => {
            if (currentAttempt === attempt) pending = null;
          });
        return pending;
      },
      disconnected: () => {
        attempt++;
        pending = null;
        id = null;
        snapshot = undefined;
        buffered.length = 0;
        overflow = false;
      },
      receive: (message) => {
        if (disposed || ("payload" in message && Object.is(message.payload, snapshot))) return;
        if (buffered.length < MAX_BOOTSTRAP_UPDATES && !overflow) buffered.push(message);
        else {
          buffered.length = 0;
          overflow = true;
        }
        for (const observer of observers) deliver(observer, message);
      },
      release: () => {
        if (teardown) return teardown;
        disposed = true;
        this.queued.delete(registration);
        signal?.removeEventListener("abort", abort);
        observers.clear();
        buffered.length = 0;
        if (id) this.routes.delete(id);
        reject(new Error("Subscription released"));
        teardown = (async () => {
          try {
            await pending;
            await releaseId();
          } finally {
            this.registrations.delete(registration);
          }
        })();
        return teardown;
      },
    };
    const abort = () => {
      void handle.release().catch(this.host.failed);
    };
    const handle: OwnedSubscription<T> = {
      get subscriptionId() {
        return id;
      },
      ready,
      release: registration.release,
      subscribe: (observer) => {
        if (disposed) throw new Error("Subscription released");
        observers.add(observer);
        if (overflow) {
          overflow = false;
          snapshot = undefined;
          void releaseId()
            .then(() => {
              this.enqueue(registration);
              return undefined;
            })
            .catch(this.host.failed);
        } else {
          deliver(observer);
          for (const message of buffered) deliver(observer, message);
        }
        return () => {
          observers.delete(observer);
        };
      },
    };
    this.registrations.add(registration);
    if (signal?.aborted) abort();
    else {
      signal?.addEventListener("abort", abort, { once: true });
      this.enqueue(registration);
    }
    return handle;
  }

  owns(message: SessionOutboundMessage): boolean {
    const payload = "payload" in message ? message.payload : message;
    return (
      "subscriptionId" in payload &&
      typeof payload.subscriptionId === "string" &&
      this.routes.has(payload.subscriptionId)
    );
  }

  receive(message: SessionOutboundMessage): void {
    const payload = "payload" in message ? message.payload : message;
    if ("subscriptionId" in payload && typeof payload.subscriptionId === "string")
      this.routes.get(payload.subscriptionId)?.receive(message);
  }

  restore(): void {
    this.connected = true;
    for (const registration of this.registrations) this.queued.add(registration);
    this.reportRestoring(this.queued.size > 0);
    this.drain();
  }

  /** Resolves when the current business subscriptions finish restoring, or disconnect. */
  whenRestored(): Promise<void> {
    if (!this.queued.size && !this.active.size) return Promise.resolve();
    return new Promise((resolve) => this.idle.add(resolve));
  }

  private enqueue(registration: Registration): void {
    this.queued.add(registration);
    this.drain();
  }

  private drain(): void {
    if (!this.connected) return;
    while (this.active.size < 4 && this.queued.size) {
      let registration = this.queued.values().next().value!;
      for (const candidate of this.queued) {
        if (candidate.priority < registration.priority) registration = candidate;
        if (registration.priority === 0) break;
      }
      this.queued.delete(registration);
      const task = registration.start();
      if (!task) continue;
      this.active.set(registration, task);
      void task.finally(() => {
        if (this.active.get(registration) !== task) return;
        this.active.delete(registration);
        this.drain();
      });
    }
    if (!this.queued.size && !this.active.size) this.resolveIdle();
  }

  private resolveIdle(): void {
    this.reportRestoring(false);
    for (const resolve of this.idle) resolve();
    this.idle.clear();
  }

  private reportRestoring(active: boolean): void {
    if (this.restoring === active) return;
    this.restoring = active;
    this.host.restoring?.(active);
  }

  disconnected(): void {
    this.connected = false;
    this.generation++;
    this.queued.clear();
    this.active.clear();
    this.resolveIdle();
    this.routes.clear();
    for (const registration of this.registrations) registration.disconnected();
  }

  async close(): Promise<void> {
    await Promise.allSettled([...this.registrations].map((registration) => registration.release()));
  }
}

function assertSubscriptionSnapshot(value: unknown): asserts value is { subscriptionId: string } {
  if (
    value &&
    typeof value === "object" &&
    "subscriptionId" in value &&
    typeof value.subscriptionId === "string" &&
    value.subscriptionId
  )
    return;
  throw new Error("The host omitted its subscription ID");
}

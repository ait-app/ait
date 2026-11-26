interface Cleanup {
  promise: Promise<void>;
  finish(error?: unknown): void;
  retry?: ReturnType<typeof setTimeout>;
}

/** Connection-local, idempotent cleanup. Resetting a transport retires all its remote IDs. */
export class SubscriptionCleanup {
  private readonly pending = new Map<string, Cleanup>();
  private generation = 0;

  constructor(
    private readonly host: {
      release(id: string, timeout: number): Promise<unknown>;
      retryable(error: unknown): boolean;
      expired(): void;
      completed(elapsedMs: number, attempts: number): void;
    },
  ) {}

  release(id: string): Promise<void> {
    const existing = this.pending.get(id);
    if (existing) return existing.promise;
    const epoch = this.generation;
    const started = Date.now();
    let attempts = 0;
    let resolve!: () => void;
    let reject!: (error: unknown) => void;
    const promise = new Promise<void>((accept, fail) => {
      resolve = accept;
      reject = fail;
    });
    const deadline = setTimeout(() => {
      if (epoch !== this.generation) return;
      // Retire the entire generation before asking the caller to reconnect once.
      this.reset();
      this.host.expired();
    }, 60_000);
    const task: Cleanup = {
      promise,
      finish: (error) => {
        clearTimeout(deadline);
        clearTimeout(task.retry);
        this.pending.delete(id);
        if (error) reject(error);
        else resolve();
      },
    };
    this.pending.set(id, task);
    const run = async () => {
      if (epoch !== this.generation) return;
      attempts++;
      try {
        await this.host.release(id, Math.min(5_000, Math.max(1, 60_000 - (Date.now() - started))));
        if (epoch !== this.generation) return;
        task.finish();
        this.host.completed(Date.now() - started, attempts);
      } catch (error) {
        if (epoch !== this.generation) return;
        if (!this.host.retryable(error)) {
          task.finish(error);
          return;
        }
        const delay = Math.min(5_000, 250 * 2 ** Math.min(attempts - 1, 5));
        task.retry = setTimeout(
          () => void run(),
          Math.min(5_000, delay * (0.8 + Math.random() * 0.4)),
        );
      }
    };
    void run();
    return promise;
  }

  reset(): void {
    this.generation++;
    for (const task of this.pending.values()) task.finish();
  }
}

import type { AgentPermissionRequest, AgentPermissionResponse } from "@ait/protocol/agent-types";
import type { AgentPermissionResolvedMessage } from "@ait/protocol/messages";

export const PERMISSION_RESPONSE_TIMEOUT_MS = 60_000;
type Resolution = AgentPermissionResolvedMessage["payload"];
export interface PermissionResponseState {
  phase: "submitting" | "uncertain" | "confirmed" | "rejected";
  startedAt: number;
  checking?: boolean;
  result?: Resolution;
}

export class PermissionOutcomeUnknownError extends Error {
  constructor() {
    super("The host has not confirmed this approval. Check its status before trying again.");
    this.name = "PermissionOutcomeUnknownError";
  }
}

/** Retain operation identity across card remounts and reconnects; never replay a submission. */
export class PermissionResponses {
  private readonly states = new Map<string, PermissionResponseState>();
  private readonly pending = new Map<string, Promise<Resolution>>();
  private readonly listeners = new Set<() => void>();
  private readonly requests = new Map<string, AgentPermissionRequest>();
  private revision = 0;

  constructor(
    private readonly host: {
      send(
        agentId: string,
        requestId: string,
        response: AgentPermissionResponse,
        timeout: number,
      ): Promise<Resolution>;
      definitive(error: unknown): boolean;
      check(agentId: string, requestId: string): Promise<Resolution | null>;
      failed?(error: unknown, unconfirmed: boolean): void;
    },
  ) {}

  snapshot(agentId: string, requestId: string): PermissionResponseState | undefined {
    return this.states.get(JSON.stringify([agentId, requestId]));
  }

  get version(): number {
    return this.revision;
  }

  remember(agentId: string, request: AgentPermissionRequest): void {
    if (this.requests.size >= 512) {
      for (const [key, state] of this.states) {
        if (state.phase !== "confirmed" && state.phase !== "rejected") continue;
        this.requests.delete(key);
        this.states.delete(key);
        if (this.requests.size < 512) break;
      }
    }
    if (this.requests.size >= 512) return;
    this.requests.set(JSON.stringify([agentId, request.id]), request);
  }

  unconfirmed(agentId: string): AgentPermissionRequest[] {
    const result: AgentPermissionRequest[] = [];
    for (const [key, request] of this.requests) {
      const state = this.states.get(key);
      if (
        key === JSON.stringify([agentId, request.id]) &&
        (state?.phase === "uncertain" || state?.phase === "submitting")
      )
        result.push(request);
    }
    return result;
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  private set(key: string, state: PermissionResponseState): void {
    this.states.set(key, state);
    this.revision++;
    for (const listener of this.listeners) listener();
  }

  submit(
    agentId: string,
    requestId: string,
    response: AgentPermissionResponse,
    timeout: number,
  ): Promise<Resolution> {
    const key = JSON.stringify([agentId, requestId]);
    const pending = this.pending.get(key);
    if (pending) return pending;
    const state = this.states.get(key);
    if (state?.result) return Promise.resolve(state.result);
    if (state?.phase === "uncertain") return Promise.reject(new PermissionOutcomeUnknownError());
    // Keep unknown outcomes until evidence resolves them. Only completed records may be evicted.
    if (!state && this.states.size >= 512) {
      for (const [id, value] of this.states) {
        if (value.phase === "confirmed" || value.phase === "rejected") {
          this.states.delete(id);
          this.requests.delete(id);
        }
        if (this.states.size < 512) break;
      }
      if (this.states.size >= 512)
        return Promise.reject(new Error("Too many unconfirmed approvals"));
    }
    const startedAt = Date.now();
    this.set(key, { phase: "submitting", startedAt });
    const task = this.host
      .send(agentId, requestId, response, timeout)
      .then((result) => {
        this.confirm(result);
        return result;
      })
      .catch((error: unknown) => {
        const result = this.states.get(key)?.result;
        if (result) return result;
        const definitive = this.host.definitive(error);
        if (definitive) this.requests.delete(key);
        this.host.failed?.(error, !definitive);
        this.set(key, { phase: definitive ? "rejected" : "uncertain", startedAt });
        if (definitive) throw error;
        throw new PermissionOutcomeUnknownError();
      })
      .finally(() => {
        this.pending.delete(key);
      });
    this.pending.set(key, task);
    return task;
  }

  confirm(result: Resolution): void {
    const key = JSON.stringify([result.agentId, result.requestId]);
    const state = this.states.get(key);
    this.requests.delete(key);
    if (state) this.set(key, { ...state, phase: "confirmed", checking: false, result });
  }

  async check(agentId: string, requestId: string): Promise<void> {
    const key = JSON.stringify([agentId, requestId]);
    const state = this.states.get(key);
    if (!state || state.phase !== "uncertain" || state.checking) return;
    this.set(key, { ...state, checking: true });
    try {
      const result = await this.host.check(agentId, requestId);
      if (result) this.confirm(result);
    } finally {
      const current = this.states.get(key)!;
      this.set(key, { ...current, checking: false });
    }
  }

  async reconnect(): Promise<void> {
    for (const [key, state] of this.states) {
      if (state.phase !== "uncertain") continue;
      const [agentId, requestId] = JSON.parse(key) as [string, string];
      await this.check(agentId, requestId).catch(() => {});
    }
  }
}

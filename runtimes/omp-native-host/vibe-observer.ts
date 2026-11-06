import * as fs from "node:fs/promises";
import * as path from "node:path";
import type { AgentSession } from "../../session/agent-session";
import { AgentRegistry, type AgentRef } from "../../registry/agent-registry";
import { parseSessionEntries } from "../../session/session-loader";
import { parseLifecycleEvent, VIBE_LIFECYCLE_CUSTOM_TYPE, type VibeSpawnLifecycleEvent } from "../../vibe/lifecycle";
import { readRpcSubagentTranscript } from "./rpc-subagents";

type ObjectValue = Record<string, unknown>;
type Output = (frame: object) => void;
type Child = { id: string; workerId: string; parentId: string; sessionFile: string; parentSessionFile: string; cwd: string; ownerId: string; createdAt: string; updatedAt: string; status: string; outcomeKnown: boolean; nativeState: string; description?: string; model?: string; modelRole?: string; thinkingLevel?: string; directWorker: boolean; screen?: ObjectValue };
const UUID = /^[a-f\d]{8}-(?:[a-f\d]{4}-){3}[a-f\d]{12}$/i;
const object = (value: unknown): ObjectValue | undefined => value && typeof value === "object" && !Array.isArray(value) ? value as ObjectValue : undefined;

/** Verified, read-only projection. It never opens/resumes a worker or writes native files. */
export class VibeObserver {
  #claims = new Map<string, VibeSpawnLifecycleEvent>();
  #children = new Map<string, Child>();
  #byAlias = new Map<string, Child>();
  #sent = new Map<string, string>();
  #outcomes = new Map<string, string>();
  #pending = new Map<string, ObjectValue[]>();
  #pendingBytes = 0;
  #refresh?: Promise<void>;
  #timer?: ReturnType<typeof setTimeout>;
  #closed = false;
  #unsubscribe: () => void;
  constructor(readonly session: AgentSession, readonly output: Output, readonly screens: () => Promise<ObjectValue[]>) {
    this.scanClaims();
    this.#unsubscribe = AgentRegistry.global().onChange(() => { this.scanClaims(); this.schedule(50); });
  }
  scanClaims() {
    const ownerId = this.session.getAgentId() ?? "main";
    const parentSessionId = this.session.sessionManager.getSessionId();
    for (const entry of this.session.sessionManager.getEntries()) {
      if (entry.type !== "custom" || entry.customType !== VIBE_LIFECYCLE_CUSTOM_TYPE) continue;
      const event = parseLifecycleEvent(entry.data);
      if (event?.action !== "spawn" || event.ownerId !== ownerId || event.parentSessionId !== parentSessionId || !/^[A-Za-z0-9_-]+$/.test(event.id) || event.childSessionFile !== `${event.id}.jsonl`) continue;
      this.#claims.set(event.id, event);
    }
  }
  ownedAlias(id: string): boolean {
    const visited = new Set<string>();
    let current: string | undefined = id;
    while (current && !visited.has(current) && visited.size < 32) {
      if (this.#claims.has(current)) return true;
      visited.add(current);
      current = AgentRegistry.global().get(current)?.parentId;
    }
    return false;
  }
  consume(frame: ObjectValue): boolean {
    if (typeof frame.type !== "string" || !frame.type.startsWith("subagent_")) return false;
    const payload = object(frame.payload);
    const source = frame.type === "subagent_progress" ? object(payload?.progress) : payload;
    const alias = source?.id;
    if (typeof alias !== "string" || !this.ownedAlias(alias)) return false;
    const status = source?.status;
    if (typeof status === "string" && ["completed", "failed", "aborted"].includes(status)) this.#outcomes.set(alias, status);
    else if (status === "started" || status === "running") this.#outcomes.delete(alias);
    if (frame.type === "subagent_event") {
      const child = this.#byAlias.get(alias);
      if (child) this.forward(frame, child);
      else {
        const size = Buffer.byteLength(JSON.stringify(frame));
        if (this.#pendingBytes + size > 8 * 1024 * 1024) throw new Error("Native Vibe observation buffer exceeded its limit.");
        this.#pendingBytes += size;
        const pending = this.#pending.get(alias) ?? [];
        pending.push(frame);
        this.#pending.set(alias, pending);
        this.schedule(20);
      }
    } else this.schedule(50);
    return true;
  }
  forward(frame: ObjectValue, child: Child) {
    this.output({ ...frame, payload: { ...object(frame.payload), id: child.id } });
  }
  schedule(delay: number) {
    if (this.#closed || this.#timer) return;
    this.#timer = setTimeout(() => {
      this.#timer = undefined;
      void this.refresh().catch(error => this.output({ type: "notice", level: "error", source: "ait-vibe-observation", message: error instanceof Error ? error.message : "Native Vibe observation failed." }));
    }, delay);
    this.#timer.unref();
  }
  async refresh() {
    if (this.#closed) return;
    if (this.#refresh) return this.#refresh;
    const refresh = this.refreshOwned();
    this.#refresh = refresh;
    try { await refresh; } finally { if (this.#refresh === refresh) this.#refresh = undefined; }
  }
  async refreshOwned() {
    this.scanClaims();
    const parentFile = this.session.sessionManager.getSessionFile();
    if (!parentFile) return;
    const rootFile = await fs.realpath(parentFile).catch((error: NodeJS.ErrnoException) => { if (error.code === "ENOENT") return undefined; throw error; });
    if (!rootFile) return;
    const screenList = await this.screens();
    const screens = new Map(screenList.map(screen => [String(screen.id), screen]));
    const registry = AgentRegistry.global();
    const candidates = new Map<string, AgentRef | undefined>();
    for (const id of this.#claims.keys()) candidates.set(id, registry.get(id));
    for (const ref of registry.list()) if (ref.kind === "sub" && this.ownedAlias(ref.id)) candidates.set(ref.id, ref);
    if (candidates.size > 256) throw new Error("Native Vibe observation exceeds 256 descendants.");
    // Verify parents before descendants; an unverified ancestor never becomes a root alias.
    for (let depth = 0; depth < 32 && candidates.size; depth++) {
      let changed = false;
      for (const [alias, ref] of candidates) {
        const claim = this.#claims.get(alias);
        const parent = claim ? undefined : ref?.parentId ? this.#byAlias.get(ref.parentId) : undefined;
        if (!claim && !parent) continue;
        const expectedParent = parent?.sessionFile ?? rootFile;
        const file = claim ? path.join(rootFile.slice(0, -6), claim.childSessionFile) : ref?.sessionFile;
        if (!file) continue;
        const child = await this.inspect(alias, file, expectedParent, parent?.id ?? this.session.sessionId, ref, screens.get(alias), !!claim);
        if (!child) continue;
        this.#children.set(child.id, child);
        this.#byAlias.set(alias, child);
        const encoded = JSON.stringify(child);
        if (this.#sent.get(child.id) !== encoded) {
          this.output({ type: "ait_vibe_worker", worker: child });
          this.#sent.set(child.id, encoded);
        }
        for (const frame of this.#pending.get(alias) ?? []) {
          this.#pendingBytes -= Buffer.byteLength(JSON.stringify(frame));
          this.forward(frame, child);
        }
        this.#pending.delete(alias);
        candidates.delete(alias);
        changed = true;
      }
      if (!changed) break;
    }
    if ([...this.#children.values()].some(child => child.nativeState === "starting" || child.nativeState === "running") || this.#pending.size) this.schedule(500);
  }
  async inspect(alias: string, file: string, expectedParent: string, parentId: string, ref: AgentRef | undefined, screen: ObjectValue | undefined, directWorker: boolean): Promise<Child | undefined> {
    let canonical: string;
    try {
      const stat = await fs.lstat(file);
      if (!stat.isFile() || stat.size > 64 * 1024 * 1024) throw new Error("Invalid native worker transcript.");
      canonical = await fs.realpath(file);
    } catch (error) { if ((error as NodeJS.ErrnoException).code === "ENOENT") return; throw error; }
    if (canonical !== path.join(expectedParent.slice(0, -6), `${alias}.jsonl`)) throw new Error("Native worker transcript escaped its verified parent artifact directory.");
    if (ref && (ref.kind !== "sub" || ref.sessionFile && await fs.realpath(ref.sessionFile) !== canonical || directWorker && ref.parentId !== (this.session.getAgentId() ?? "main"))) throw new Error("Native worker registry ownership conflicts with its transcript.");
    const prefix = await Bun.file(canonical).slice(0, 128 * 1024).text();
    const newline = prefix.lastIndexOf("\n");
    const header = newline >= 0 ? parseSessionEntries(prefix.slice(0, newline + 1)).find(entry => entry.type === "session") : undefined;
    if (!header) return;
    if (!UUID.test(header.id) || header.id === parentId || !path.isAbsolute(header.cwd) || !header.parentSession || await fs.realpath(header.parentSession) !== expectedParent) throw new Error("Native worker header does not verify the claimed immediate parent.");
    const previous = this.#children.get(header.id);
    let status = this.#outcomes.get(alias);
    let thinkingLevel = previous?.thinkingLevel;
    if (!status && (ref?.status === "idle" || ref?.status === "parked" || !ref)) {
      const transcript = await readRpcSubagentTranscript(canonical);
      const last = transcript.messages.findLast(message => message.role === "assistant");
      if (last?.role === "assistant") status = last.stopReason === "error" ? "failed" : last.stopReason === "aborted" ? "aborted" : ["stop", "length"].includes(last.stopReason) ? "completed" : undefined;
      const thinking = transcript.entries.findLast(entry => entry.type === "thinking_level_change");
      if (thinking?.type === "thinking_level_change") thinkingLevel = thinking.thinkingLevel;
    }
    const nativeState = String(screen?.state ?? ref?.status ?? "unknown");
    if (screen?.killed === true || ref?.status === "aborted") status = "aborted";
    const running = nativeState === "starting" || nativeState === "running";
    return { id: header.id, workerId: alias, parentId, sessionFile: canonical, parentSessionFile: expectedParent,
      cwd: header.cwd, ownerId: this.session.getAgentId() ?? "main", createdAt: header.timestamp,
      updatedAt: new Date(typeof screen?.lastActivityAt === "number" ? screen.lastActivityAt : ref?.lastActivity ?? Date.parse(header.timestamp)).toISOString(),
      status: running ? "running" : status === "completed" ? "completed" : status === "failed" ? "failed" : status === "aborted" ? "canceled" : "unknown", outcomeKnown: running || !!status,
      nativeState, description: typeof screen?.turnMessage === "string" ? screen.turnMessage : ref?.activity,
      model: typeof screen?.model === "string" ? screen.model : ref?.history?.resolvedModel,
      modelRole: ref?.history?.modelRole, thinkingLevel, directWorker, screen };
  }
  list(): Child[] { return [...this.#children.values()]; }
  async resolve(id: string): Promise<string> {
    await this.refresh();
    const child = this.#children.get(id);
    if (!child?.directWorker || child.parentId !== this.session.sessionId || child.parentSessionFile !== await fs.realpath(this.session.sessionManager.getSessionFile()!)) throw new Error("Worker does not belong to this native Vibe parent scope.");
    const ref = AgentRegistry.global().get(child.workerId);
    if (!ref?.sessionFile || ref.parentId !== child.ownerId || await fs.realpath(ref.sessionFile) !== child.sessionFile) throw new Error("Native worker identity changed; refresh its owner roster.");
    return child.workerId;
  }
  close() {
    this.#closed = true;
    this.#unsubscribe();
    if (this.#timer) clearTimeout(this.#timer);
  }
}

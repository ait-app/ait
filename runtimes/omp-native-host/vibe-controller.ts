import { randomUUID } from "node:crypto";
import { formatModelString } from "../../config/model-resolver";
import type { AgentSession } from "../../session/agent-session";
import type { InternalAvailableSlashCommand } from "../../slash-commands/available-commands";
import { VibeSessionRegistry, type VibeOwnerScope, type VibeParentSession } from "../../vibe/runtime";
import { VibeObserver } from "./ait-vibe-observer";

/** The bridge is shipped only with this exact, integrity-locked official SDK. */
export const AIT_NATIVE_HOST = Object.freeze({ bridgeVersion: 1, sdkVersion: "18.17.0", bunVersion: "1.4.4", capabilities: { vibe: true, vibeWorkerObservation: true, vibeWorkerControl: true } });
type Output = (frame: object) => void;
type Input = { type: string; id?: string; operation?: string; sessionId?: string; message?: string; cli?: string; name?: string; prompt?: string; sessions?: string[]; timeout?: number; enabled?: boolean };
const MUTATIONS = new Set(["new_session", "switch_session", "open_session", "branch", "fork", "navigate_tree", "handoff", "move_session", "delete_session", "reset_loop"]);
const OPERATIONS = new Set(["list", "spawn", "send", "wait", "kill", "toggle"]);

/** Thin mode/RPC glue. ToolSession, workers, execution and persistence remain official SDK-owned. */
export class AitVibeController {
  readonly observer: VibeObserver;
  #scope?: VibeOwnerScope;
  #previousTools?: string[];
  #entry?: Promise<void>;
  #exit?: Promise<void>;
  #enabled = false;
  constructor(readonly session: AgentSession, readonly output: Output) {
    this.observer = new VibeObserver(session, output, () => this.screens());
  }
  parent(): VibeParentSession {
    const session = this.session;
    return { getAgentId: () => session.getAgentId() ?? null, getSessionId: () => session.sessionManager.getSessionId(),
      getSessionFile: () => session.sessionManager.getSessionFile() ?? null, sessionManager: session.sessionManager,
      asyncJobManager: session.asyncJobManager, settings: session.settings,
      getActiveModelString: () => session.model ? formatModelString(session.model) : undefined };
  }
  state() { return { enabled: this.#enabled, entering: !!this.#entry, exiting: !!this.#exit }; }
  async initialize() {
    await VibeSessionRegistry.global().rehydrate(this.parent());
    if (this.session.sessionManager.buildSessionContext().mode === "vibe") await this.enter(false);
    await this.observer.refresh();
  }
  commands(commands: InternalAvailableSlashCommand[]): InternalAvailableSlashCommand[] {
    // A real registered extension/custom command retains its native owner.
    if (commands.some(command => command.name === "vibe")) return commands;
    return [...commands, { name: "vibe", source: "builtin", description: "Toggle native Vibe director mode; direct persistent fast/good workers", input: { hint: "[initial prompt]" },
      aitNativeHost: AIT_NATIVE_HOST } as InternalAvailableSlashCommand];
  }
  assertSessionChange() {
    if (this.#enabled || this.#entry || this.#exit) throw new Error("Exit Vibe mode before changing the native conversation.");
  }
  assertRequest(command: Input) {
    if (!(this.#enabled || this.#entry || this.#exit)) return;
    if (MUTATIONS.has(command.type) || command.type === "goal") this.assertSessionChange();
  }
  async input(text: string, commands: readonly InternalAvailableSlashCommand[]): Promise<string | null | undefined> {
    const match = /^\/([^\s:]+)(?:\s+|:)?([\s\S]*)$/.exec(text.trim());
    if (!match) return;
    const command = commands.find(command => command.name === match[1] || command.aliases?.includes(match[1]));
    if (command?.source !== "builtin") return;
    if (match[1] !== "vibe") {
      if ((this.#enabled || this.#entry || this.#exit) && ["plan", "plan-review", "goal", "guided-goal", "new", "delete", "fork", "move", "handoff", "resetloop"].includes(match[1])) this.assertSessionChange();
      return;
    }
    if (this.#exit) { await this.#exit; return null; }
    // Concurrent entries join the activation, rather than toggling the unfinished entry off.
    if (this.#entry) await this.#entry;
    else if (this.#enabled) { await this.exit(); return null; }
    else await this.enter();
    return match[2].trim() || null;
  }
  async enter(persist = true) {
    if (this.#entry) return this.#entry;
    if (this.#enabled) return;
    if (this.#exit) await this.#exit;
    const context = this.session.sessionManager.buildSessionContext();
    if (["plan", "plan_paused", "goal", "goal_paused"].includes(context.mode ?? "none") || this.session.getGoalModeState()?.enabled)
      throw new Error("Exit the active or paused plan/goal before entering Vibe mode.");
    const previousTools = this.session.getEnabledToolNames();
    const parent = this.parent();
    const registry = VibeSessionRegistry.global();
    const scope = registry.ownerScope(parent);
    const entry = (async () => {
      registry.activateScope(scope);
      const tools = this.session.hasBuiltInTool("todo") ? ["read", "todo"] : ["read"];
      await this.session.activateVibeTools(tools);
      this.#previousTools = previousTools;
      this.#scope = scope;
      this.#enabled = true;
      this.session.setVibeModeState({ enabled: true });
      if (this.session.isStreaming) await this.session.sendVibeModeContext({ deliverAs: "steer" });
      if (persist) this.session.sessionManager.appendModeChange("vibe", { previousTools });
      this.output({ type: "command_output", text: "Vibe mode enabled. Native fast/good workers are directed with read, optional parent Todo and the five Vibe tools." });
      this.publishState();
    })();
    this.#entry = entry;
    try { await entry; } finally { if (this.#entry === entry) this.#entry = undefined; }
  }
  async exit() {
    if (this.#exit) return this.#exit;
    if (this.#entry) await this.#entry;
    if (!this.#enabled) return;
    const exit = (async () => {
      let killed = 0;
      await this.session.runModeExitTeardown(async () => {
        if (this.session.isStreaming) await this.session.abort();
        killed = await VibeSessionRegistry.global().killAll(this.parent(), this.#scope);
        // Snapshot terminal workers while the official read-only Vibe tool still exists.
        await this.observer.refresh();
        await this.session.deactivateVibeTools(this.#previousTools ?? []);
        this.session.setVibeModeState(undefined);
      });
      this.#enabled = false;
      this.#scope = undefined;
      this.#previousTools = undefined;
      this.output({ type: "command_output", text: `Vibe mode disabled. ${killed} native worker session(s) terminated.` });
      this.publishState();
    })();
    this.#exit = exit;
    try { await exit; } finally { if (this.#exit === exit) this.#exit = undefined; }
  }
  publishState() { this.output({ type: "ait_vibe_state", state: this.state(), runtime: AIT_NATIVE_HOST }); }
  async screens(): Promise<Record<string, unknown>[]> {
    const tool = this.session.getToolByName("vibe_list");
    if (!tool) return [];
    const result = await tool.execute(`ait-observe-${randomUUID()}`, {});
    return (result.details as { screens?: Record<string, unknown>[] } | undefined)?.screens ?? [];
  }
  async control(command: Input): Promise<object> {
    const { operation } = command;
    if (!operation || !OPERATIONS.has(operation)) throw new Error("Unknown native Vibe operation.");
    if (operation === "toggle") {
      if (typeof command.enabled === "boolean") {
        if (command.enabled) await this.enter(); else await this.exit();
      } else if (this.#enabled) await this.exit(); else await this.enter();
      return this.state();
    }
    if (this.#entry) await this.#entry;
    if (!this.#enabled || this.#exit) throw new Error("Enter Vibe mode before controlling its workers.");
    if (operation === "list") { await this.observer.refresh(); return { workers: this.observer.list(), state: this.state() }; }
    const params: Record<string, unknown> = {};
    if (operation === "spawn") {
      if (!["fast", "good"].includes(command.cli ?? "") || typeof command.prompt !== "string" || !command.prompt.trim() || command.prompt.length > 65536 || command.name !== undefined && (typeof command.name !== "string" || command.name.length > 48)) throw new Error("Vibe spawn requires fast/good and a nonempty bounded prompt/name.");
      Object.assign(params, { cli: command.cli, prompt: command.prompt, ...(command.name === undefined ? {} : { name: command.name }) });
    } else if (operation === "send" || operation === "kill") {
      if (typeof command.sessionId !== "string") throw new Error("A verified native worker session ID is required.");
      params.session = await this.observer.resolve(command.sessionId);
      if (operation === "send") {
        if (typeof command.message !== "string" || !command.message.trim() || command.message.length > 65536) throw new Error("A nonempty bounded worker message is required.");
        params.message = command.message;
      }
    } else if (operation === "wait") {
      if (command.sessions !== undefined && (!Array.isArray(command.sessions) || command.sessions.length > 256 || !command.sessions.every(id => typeof id === "string"))) throw new Error("Invalid native worker session IDs.");
      if (command.sessions) params.sessions = await Promise.all(command.sessions.map(id => this.observer.resolve(id)));
      if (command.timeout !== undefined) {
        if (typeof command.timeout !== "number" || !Number.isFinite(command.timeout) || command.timeout <= 0 || command.timeout > 600) throw new Error("Vibe wait timeout must be between 0 and 600 seconds.");
        params.timeout = command.timeout;
      }
    }
    // The SDK's enabled-tool accessor applies its native permission gate and retains
    // the factory's complete ToolSession (private roles, accounts, approval and jobs).
    const tool = this.session.getToolForEvalBridge(`vibe_${operation}`);
    if (!tool) throw new Error("The native Vibe tool is not enabled.");
    const result = await tool.execute(`ait-control-${randomUUID()}`, params);
    await this.observer.refresh();
    return result;
  }
  close() { this.observer.close(); }
}

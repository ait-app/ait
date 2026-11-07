/** Glue between the Ait native host RPC adapter and the official session. */
import type { AgentSession } from "../../session/agent-session";
import type { InternalAvailableSlashCommand } from "../../slash-commands/available-commands";
import { AitVibeController } from "./ait-vibe-controller";

/** One `ait_vibe_control` frame from Ait; the controller validates every field. */
export type AitVibeControlFrame = {
  type?: string;
  id?: string;
  operation?: unknown;
  sessionId?: unknown;
  message?: unknown;
  cli?: unknown;
  name?: unknown;
  prompt?: unknown;
  sessions?: unknown;
  timeout?: unknown;
  enabled?: unknown;
};

const OPERATIONS: Record<string, true> = {
  list: true,
  spawn: true,
  send: true,
  wait: true,
  kill: true,
  toggle: true,
};

/**
 * Owns the single Vibe controller for this RPC connection. Constructed only for a patched
 * `rpc-ui` launch, so a plain RPC host never gains Vibe behavior.
 */
export class AitNativeHostBridge {
  readonly #controller: AitVibeController;
  constructor(session: AgentSession, output: (frame: object) => void) {
    this.#controller = new AitVibeController(session, output);
  }
  /** Rehydrate persisted scopes and the current native worker roster. */
  async initialize(): Promise<void> {
    await this.#controller.initialize();
  }
  /** Add the native `vibe` command unless an extension or file command already owns it. */
  commands(commands: InternalAvailableSlashCommand[]): InternalAvailableSlashCommand[] {
    return this.#controller.commands(commands);
  }
  /** Toggle Vibe mode and return the inline directive, `null` for a mode-only change. */
  async input(
    text: string,
    commands: readonly InternalAvailableSlashCommand[],
  ): Promise<string | null | undefined> {
    return this.#controller.input(text, commands);
  }
  /** Refuse a conversation change while Vibe mode owns the native session. */
  assertSessionChange(): void {
    this.#controller.assertSessionChange();
  }
  /** Refuse a mutating native command while Vibe mode owns the native session. */
  assertRequest(frame: AitVibeControlFrame): void {
    this.#controller.assertRequest(
      frame as unknown as Parameters<AitVibeController["assertRequest"]>[0],
    );
  }
  /** Execute one validated Vibe control; worker identity is resolved by the observer. */
  async control(frame: AitVibeControlFrame): Promise<object> {
    const operation = frame.operation;
    if (typeof operation !== "string" || OPERATIONS[operation] !== true) {
      throw new Error("Unknown native Vibe operation.");
    }
    return this.#controller.control(
      frame as unknown as Parameters<AitVibeController["control"]>[0],
    );
  }
  /** Release observation timers without touching native worker sessions. */
  close(): void {
    this.#controller.close();
  }
}

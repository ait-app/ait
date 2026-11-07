import type { WorkspaceIsolation } from "@/create-agent-preferences/preferences";

export interface IsolationAvailability {
  canCreateWorktree: boolean;
  canSandboxWorktree: boolean;
}

/** True for every isolation level that places the workspace in a new Paseo-owned worktree. */
export function createsWorktree(isolation: WorkspaceIsolation): boolean {
  return isolation !== "local";
}

/**
 * Downgrade a remembered choice to the strongest level the selected host and project support.
 * The trigger always shows the effective level, so a fallback is never hidden from the user.
 */
export function resolveEffectiveIsolation(
  isolation: WorkspaceIsolation,
  availability: IsolationAvailability,
): WorkspaceIsolation {
  if (!availability.canCreateWorktree || isolation === "local") return "local";
  if (isolation === "sandboxed-worktree" && !availability.canSandboxWorktree) return "worktree";
  return isolation;
}

/** Isolation levels offered in the picker, weakest first. */
export function availableIsolations(availability: IsolationAvailability): WorkspaceIsolation[] {
  if (!availability.canCreateWorktree) return ["local"];
  if (!availability.canSandboxWorktree) return ["local", "worktree"];
  return ["local", "worktree", "sandboxed-worktree"];
}

/** Map a picker option id back to an isolation level, treating unknown ids as local. */
export function parseIsolationOptionId(id: string): WorkspaceIsolation {
  return id === "worktree" || id === "sandboxed-worktree" ? id : "local";
}

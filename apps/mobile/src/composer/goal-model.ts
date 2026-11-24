export type GoalAction = "edit" | "pause" | "resume" | "clear";
export interface GoalSnapshot {
  objective: string;
  status: string;
  actions: Record<GoalAction, boolean>;
  tokensUsed?: number;
  tokenBudget?: number;
}

/** Only structured adapter facts create a Goal pill; prose and tool names do not. */
export function readGoal(value: unknown): GoalSnapshot | null {
  if (!value || typeof value !== "object") return null;
  const raw = value as Record<string, unknown>;
  if (
    typeof raw.objective !== "string" ||
    !raw.objective.trim() ||
    raw.objective.length > 65536 ||
    typeof raw.status !== "string"
  )
    return null;
  const actions =
    raw.actions && typeof raw.actions === "object" ? (raw.actions as Record<string, unknown>) : {};
  const number = (value: unknown) =>
    typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : undefined;
  return {
    objective: raw.objective,
    status: [
      "active",
      "paused",
      "completed",
      "budgetLimited",
      "blocked",
      "usageLimited",
      "failed",
    ].includes(raw.status)
      ? raw.status
      : "unknown",
    actions: {
      edit: actions.edit === true,
      pause: actions.pause === true,
      resume: actions.resume === true,
      clear: actions.clear === true,
    },
    tokensUsed: number(raw.tokensUsed),
    tokenBudget: number(raw.tokenBudget),
  };
}

export function goalCommand(action: GoalAction, objective?: string): string {
  return action === "edit" ? `/goal set ${objective?.trim() ?? ""}` : `/goal ${action}`;
}

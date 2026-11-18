import type { ScheduleSummary } from "@ait/protocol/schedule/types";

/** Return runnable or paused schedules that keep an existing Agent as their target. */
export function schedulesUsingAgent(
  schedules: readonly ScheduleSummary[],
  agentId: string,
): ScheduleSummary[] {
  const normalizedAgentId = agentId.trim();
  if (!normalizedAgentId) {
    return [];
  }

  return schedules.filter(
    (schedule) =>
      schedule.status !== "completed" &&
      schedule.target.type === "agent" &&
      schedule.target.agentId === normalizedAgentId,
  );
}

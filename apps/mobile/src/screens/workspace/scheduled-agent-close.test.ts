import type { ScheduleSummary } from "@ait/protocol/schedule/types";
import { describe, expect, it } from "vitest";
import { schedulesUsingAgent } from "./scheduled-agent-close";

const AGENT_ID = "11111111-1111-4111-8111-111111111111";

function schedule(
  overrides: Partial<ScheduleSummary> & Pick<ScheduleSummary, "target">,
): ScheduleSummary {
  const { target, ...rest } = overrides;
  return {
    id: "schedule-1",
    workspaceId: "workspace-1",
    name: "Daily check",
    prompt: "Check the workspace",
    cadence: { type: "every", everyMs: 60_000 },
    target,
    status: "active",
    createdAt: "2026-01-01T00:00:00.000Z",
    updatedAt: "2026-01-01T00:00:00.000Z",
    nextRunAt: "2026-01-01T00:01:00.000Z",
    lastRunAt: null,
    pausedAt: null,
    expiresAt: null,
    maxRuns: null,
    ...rest,
  };
}

describe("schedulesUsingAgent", () => {
  it("finds active and paused schedules targeting the agent", () => {
    const active = schedule({ target: { type: "agent", agentId: AGENT_ID } });
    const paused = schedule({
      id: "schedule-2",
      status: "paused",
      target: { type: "agent", agentId: AGENT_ID },
    });

    expect(schedulesUsingAgent([active, paused], AGENT_ID)).toEqual([active, paused]);
  });

  it("ignores completed, new-agent, and other-agent schedules", () => {
    const completed = schedule({
      id: "completed",
      status: "completed",
      target: { type: "agent", agentId: AGENT_ID },
    });
    const newAgent = schedule({
      id: "new-agent",
      target: { type: "new-agent", config: { provider: "codex" } },
    });
    const otherAgent = schedule({
      id: "other-agent",
      target: { type: "agent", agentId: "22222222-2222-4222-8222-222222222222" },
    });

    expect(schedulesUsingAgent([completed, newAgent, otherAgent], AGENT_ID)).toEqual([]);
  });

  it("does not match a blank agent id", () => {
    const target = schedule({ target: { type: "agent", agentId: AGENT_ID } });

    expect(schedulesUsingAgent([target], "  ")).toEqual([]);
  });
});

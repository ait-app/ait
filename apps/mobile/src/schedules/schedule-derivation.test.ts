import type { ScheduleSummary } from "@ait/protocol/schedule/types";
import { describe, expect, it } from "vitest";
import {
  resolveSchedule,
  scheduleBucket,
  scheduleEndTime,
  type ScheduleTargetAgent,
} from "./schedule-derivation";

const NOW = Date.parse("2026-07-02T00:00:00.000Z");
const AGENT_ID = "00000000-0000-4000-8000-000000000000";

function makeSchedule(overrides: Partial<ScheduleSummary>): ScheduleSummary {
  return {
    id: "schedule-1",
    name: "Nightly",
    prompt: "Run the task",
    cadence: { type: "every", everyMs: 60_000 },
    target: { type: "new-agent", config: { provider: "codex", cwd: "/tmp/project" } },
    status: "active",
    createdAt: "2026-07-01T00:00:00.000Z",
    updatedAt: "2026-07-01T00:00:00.000Z",
    nextRunAt: "2026-07-02T01:00:00.000Z",
    lastRunAt: null,
    pausedAt: null,
    expiresAt: null,
    maxRuns: null,
    ...overrides,
  };
}

function resolve(
  schedule: ScheduleSummary,
  options?: {
    agents?: [string, ScheduleTargetAgent][];
    projects?: [string, string][];
    agentDataLoaded?: boolean;
  },
) {
  return resolveSchedule({
    schedule,
    serverId: "host-1",
    now: NOW,
    agentsByKey: new Map(options?.agents ?? []),
    projectNameByCwd: new Map(options?.projects ?? []),
    agentDataLoaded: options?.agentDataLoaded ?? true,
  });
}

describe("resolveSchedule state", () => {
  it("keeps active and paused schedules runnable", () => {
    expect(resolve(makeSchedule({ status: "active" })).state).toBe("active");
    expect(resolve(makeSchedule({ status: "paused" })).state).toBe("paused");
    expect(scheduleBucket("active")).toBe("runnable");
    expect(scheduleBucket("paused")).toBe("runnable");
  });

  it("treats a past expiresAt as expired regardless of status", () => {
    const result = resolve(
      makeSchedule({ status: "active", expiresAt: "2026-07-01T00:00:00.000Z" }),
    );
    expect(result.state).toBe("expired");
    expect(result.bucket).toBe("ended");
  });

  it("ignores an unparseable expiresAt", () => {
    expect(resolve(makeSchedule({ expiresAt: "not-a-date" })).state).toBe("active");
  });

  it("derives finished only from completed-and-not-expired", () => {
    expect(resolve(makeSchedule({ status: "completed" })).state).toBe("finished");
    expect(
      resolve(makeSchedule({ status: "completed", expiresAt: "2026-07-01T00:00:00.000Z" })).state,
    ).toBe("expired");
  });

  it("marks an agent target gone when the client has no such agent", () => {
    const schedule = makeSchedule({ target: { type: "agent", agentId: AGENT_ID } });
    expect(resolve(schedule).state).toBe("targetGone");
    expect(resolve(schedule).bucket).toBe("ended");
  });

  it("does not claim gone before the agent directory has loaded", () => {
    const schedule = makeSchedule({ target: { type: "agent", agentId: AGENT_ID } });
    expect(resolve(schedule, { agentDataLoaded: false }).state).toBe("active");
  });

  it("prefers target-gone over the raw paused/completed status for a live agent target", () => {
    const paused = makeSchedule({
      status: "paused",
      target: { type: "agent", agentId: AGENT_ID },
    });
    expect(resolve(paused).state).toBe("targetGone");
  });

  it("never claims a new-agent cwd is gone", () => {
    expect(resolve(makeSchedule({ status: "active" })).state).toBe("active");
  });
});

describe("resolveSchedule target line", () => {
  it("names an agent target by its client title and provider", () => {
    const schedule = makeSchedule({ target: { type: "agent", agentId: AGENT_ID } });
    const result = resolve(schedule, {
      agents: [[`host-1:${AGENT_ID}`, { title: "Fix build", provider: "claude" }]],
    });
    expect(result.target).toEqual({ label: "Fix build", provider: "claude" });
    expect(result.state).toBe("active");
  });

  it("falls back to Untitled agent when the agent has no title", () => {
    const schedule = makeSchedule({ target: { type: "agent", agentId: AGENT_ID } });
    const result = resolve(schedule, {
      agents: [[`host-1:${AGENT_ID}`, { title: "  ", provider: "codex" }]],
    });
    expect(result.target.label).toBe("Untitled agent");
  });

  it("labels a gone agent target as unavailable with no glyph", () => {
    const schedule = makeSchedule({ target: { type: "agent", agentId: AGENT_ID } });
    expect(resolve(schedule).target).toEqual({ label: "Agent unavailable", provider: null });
  });

  it("names a new-agent cwd by matched project, else the shortened path", () => {
    const matched = makeSchedule({
      target: { type: "new-agent", config: { provider: "codex", cwd: "/tmp/project" } },
    });
    expect(resolve(matched, { projects: [["host-1:/tmp/project", "My Project"]] }).target).toEqual({
      label: "My Project",
      provider: "codex",
    });

    const unmatched = makeSchedule({
      target: { type: "new-agent", config: { provider: "codex", cwd: "/Users/alex/work/api" } },
    });
    expect(resolve(unmatched).target).toEqual({ label: "~/work/api", provider: "codex" });
  });
});

it("labels scoped tasks with their owning workspace and existing agent", () => {
  const input = {
    serverId: "host-1",
    now: NOW,
    agentsByKey: new Map([[`host-1:${AGENT_ID}`, { title: "Reviewer", provider: "codex" }]]),
    projectNameByCwd: new Map<string, string>(),
    workspaceNameByKey: new Map([["host-1:wks-1", "Feature workspace"]]),
    agentDataLoaded: true,
  };
  expect(
    resolveSchedule({ ...input, schedule: makeSchedule({ workspaceId: "wks-1" }) }).target.label,
  ).toBe("Feature workspace");
  expect(
    resolveSchedule({
      ...input,
      schedule: makeSchedule({
        workspaceId: "wks-1",
        target: { type: "agent", agentId: AGENT_ID },
      }),
    }).target.label,
  ).toBe("Feature workspace · Reviewer");
});

describe("scheduleEndTime", () => {
  it("uses execution completion rather than creation or a later edit for finished schedules", () => {
    const schedule = makeSchedule({
      status: "completed",
      lastRunAt: "2026-07-01T12:00:00Z",
      updatedAt: "2026-07-01T18:00:00Z",
    });
    expect(scheduleEndTime(schedule, "finished")).toBe(Date.parse("2026-07-01T12:00:00Z"));
  });

  it("uses expiry when the last run predates it, including schedules that never ran", () => {
    for (const lastRunAt of [null, "2026-07-01T12:00:00Z"]) {
      const schedule = makeSchedule({
        status: "completed",
        lastRunAt,
        expiresAt: "2026-07-01T18:00:00Z",
        updatedAt: "2026-07-01T18:05:00Z",
      });
      expect(scheduleEndTime(schedule, "expired")).toBe(Date.parse("2026-07-01T18:00:00Z"));
    }
  });

  it("uses the actual run end when execution finishes after expiry", () => {
    const schedule = makeSchedule({
      status: "completed",
      lastRunAt: "2026-07-01T19:00:00Z",
      expiresAt: "2026-07-01T18:00:00Z",
      updatedAt: "2026-07-01T19:00:00Z",
    });
    expect(scheduleEndTime(schedule, "expired")).toBe(Date.parse("2026-07-01T19:00:00Z"));
  });

  it("keeps an earlier completion time after the unused expiry date passes", () => {
    const schedule = makeSchedule({
      status: "completed",
      lastRunAt: "2026-07-01T12:00:00Z",
      updatedAt: "2026-07-01T12:00:00Z",
      expiresAt: "2026-07-01T18:00:00Z",
    });
    expect(scheduleEndTime(schedule, "expired")).toBe(Date.parse("2026-07-01T12:00:00Z"));
  });

  it("falls back to the last known update when a completion or target-removal time is absent", () => {
    const schedule = makeSchedule({
      lastRunAt: "2026-07-01T12:00:00Z",
      updatedAt: "2026-07-01T18:00:00Z",
    });
    expect(scheduleEndTime(schedule, "targetGone")).toBe(Date.parse("2026-07-01T18:00:00Z"));
    expect(scheduleEndTime({ ...schedule, lastRunAt: null, status: "completed" }, "finished")).toBe(
      Date.parse("2026-07-01T18:00:00Z"),
    );
  });
});

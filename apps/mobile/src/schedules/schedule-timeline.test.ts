import { describe, expect, it } from "vitest";
import type { ScheduleSummary } from "@ait/protocol/schedule/types";
import { buildScheduleTimeline } from "./schedule-timeline";
const now = new Date(2030, 0, 1, 9).getTime();
const iso = (hours: number) => new Date(now + hours * 3_600_000).toISOString();
function task(
  id: string,
  next: string | null,
  status: ScheduleSummary["status"] = "active",
): ScheduleSummary {
  return {
    id,
    workspaceId: "owner",
    name: id,
    prompt: "Review",
    cadence: { type: "every", everyMs: 3_600_000 },
    target: { type: "agent", agentId: "11111111-1111-4111-8111-111111111111" },
    status,
    maxRuns: 1,
    createdAt: iso(-1),
    updatedAt: iso(-1),
    nextRunAt: next,
    lastRunAt: null,
    pausedAt: null,
    expiresAt: null,
  };
}
describe("schedule timeline", () => {
  it("orders upcoming tasks by actual next trigger and groups local dates", () => {
    const input = [task("tomorrow", iso(24)), task("later", iso(6)), task("earlier", iso(1))];
    const groups = buildScheduleTimeline(input, "upcoming", now);
    expect(groups.map((group) => group.kind)).toEqual(["today", "tomorrow"]);
    expect(groups.flatMap((group) => group.events.map((event) => event.schedule.id))).toEqual([
      "earlier",
      "later",
      "tomorrow",
    ]);
    expect(input.map((schedule) => schedule.id)).toEqual(["tomorrow", "later", "earlier"]);
  });
  it("keeps due tasks visible and excludes stopped or unscheduled tasks", () => {
    const groups = buildScheduleTimeline(
      [
        task("due", iso(-1)),
        task("missing", null),
        task("paused", iso(2), "paused"),
        task("ended", iso(3), "completed"),
      ],
      "upcoming",
      now,
    );
    expect(groups).toHaveLength(1);
    expect(groups[0]?.kind).toBe("due");
    expect(groups[0]?.events[0]?.schedule.id).toBe("due");
  });
  it("does not invent trigger times for paused tasks", () => {
    const groups = buildScheduleTimeline([task("paused", null, "paused")], "paused", now);
    expect(groups[0]).toMatchObject({ kind: "paused", events: [{ occursAt: null }] });
  });
  it("sorts finished tasks by their last execution and supplies a date for tasks without runs", () => {
    const groups = buildScheduleTimeline(
      [
        { ...task("old", null, "completed"), lastRunAt: iso(-2) },
        { ...task("recent", null, "completed"), lastRunAt: iso(-1) },
      ],
      "completed",
      now,
    );
    expect(groups.flatMap((group) => group.events.map((event) => event.schedule.id))).toEqual([
      "recent",
      "old",
    ]);
    expect(
      buildScheduleTimeline([task("expired", null, "completed")], "completed", now)[0]?.events[0]
        ?.occursAt,
    ).toBe(iso(-1));
  });
});

import { describe, expect, it } from "vitest";
import type { ScheduleSummary } from "@ait/protocol/schedule/types";
import { buildScheduleTimeline, paginateScheduleTimeline } from "./schedule-timeline";
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

describe("paginateScheduleTimeline", () => {
  const ids = (page: ReturnType<typeof paginateScheduleTimeline>) =>
    page.groups.flatMap((group) => group.events.map((event) => event.schedule.id));
  const pad = (index: number) => String(index).padStart(2, "0");
  const today = Array.from({ length: 30 }, (_, index) => task(`a-${pad(index)}`, iso(1)));
  const later = Array.from({ length: 15 }, (_, index) => task(`b-${pad(index)}`, iso(72)));
  const groups = buildScheduleTimeline([...today, ...later], "upcoming", now);

  it("limits a page to 20 tasks and repeats a split group's heading", () => {
    const first = paginateScheduleTimeline(groups, 1);
    expect(first.pageCount).toBe(3);
    expect(ids(first)).toHaveLength(20);
    const second = paginateScheduleTimeline(groups, 2);
    expect(ids(second)[0]).toBe("a-20");
    expect(ids(second)[10]).toBe("b-00");
    expect(second.groups.map((group) => group.events.length)).toEqual([10, 10]);
    expect(second.groups[0].key).toBe(first.groups[0].key);
  });

  it("clamps a page past the end after tasks disappear", () => {
    const page = paginateScheduleTimeline(groups, 9);
    expect(page.page).toBe(3);
    expect(ids(page)).toHaveLength(5);
  });

  it("keeps a single page for an empty or exactly full list", () => {
    expect(paginateScheduleTimeline([], 1)).toEqual({ groups: [], page: 1, pageCount: 1 });
    const exact = buildScheduleTimeline(today.slice(0, 20), "upcoming", now);
    expect(paginateScheduleTimeline(exact, 1).pageCount).toBe(1);
  });
});

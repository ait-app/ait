import type { ScheduleSummary } from "@ait/protocol/schedule/types";

export type ScheduleTimelineFilter = "upcoming" | "paused" | "completed";
export interface ScheduleTimelineEvent {
  schedule: ScheduleSummary;
  occursAt: string | null;
}
export interface ScheduleTimelineGroup {
  key: string;
  kind: "due" | "today" | "tomorrow" | "date" | "paused";
  date: string | null;
  events: ScheduleTimelineEvent[];
}

function localDay(time: number): string {
  const date = new Date(time);
  return `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`;
}

/** Sort upcoming executions by the server's next trigger, without approximating recurrence rules. */
export function buildScheduleTimeline(
  schedules: readonly ScheduleSummary[],
  filter: ScheduleTimelineFilter,
  now = Date.now(),
): ScheduleTimelineGroup[] {
  const tomorrow = new Date(now);
  tomorrow.setDate(tomorrow.getDate() + 1);
  const todayKey = localDay(now);
  const tomorrowKey = localDay(tomorrow.getTime());
  const events = schedules
    .filter((schedule) => schedule.status === (filter === "upcoming" ? "active" : filter))
    .map((schedule) => ({
      schedule,
      occursAt:
        filter === "upcoming"
          ? schedule.nextRunAt
          : filter === "completed"
            ? (schedule.lastRunAt ?? schedule.updatedAt)
            : null,
    }))
    .filter(
      (event) =>
        filter !== "upcoming" ||
        (event.occursAt !== null && Number.isFinite(Date.parse(event.occursAt))),
    )
    .sort((a, b) => {
      const aTime = a.occursAt ? Date.parse(a.occursAt) : Date.parse(a.schedule.updatedAt);
      const bTime = b.occursAt ? Date.parse(b.occursAt) : Date.parse(b.schedule.updatedAt);
      return (
        (filter === "upcoming" ? aTime - bTime : bTime - aTime) ||
        a.schedule.id.localeCompare(b.schedule.id)
      );
    });
  const groups = new Map<string, ScheduleTimelineGroup>();
  for (const event of events) {
    const time = event.occursAt ? Date.parse(event.occursAt) : null;
    const day = time !== null ? localDay(time) : "paused";
    const kind =
      time === null
        ? "paused"
        : filter === "upcoming" && time <= now
          ? "due"
          : day === todayKey
            ? "today"
            : day === tomorrowKey
              ? "tomorrow"
              : "date";
    const key = kind === "due" ? "due" : day;
    const group: ScheduleTimelineGroup = groups.get(key) ?? {
      key,
      kind,
      date: event.occursAt,
      events: [],
    };
    group.events.push(event);
    groups.set(key, group);
  }
  return [...groups.values()];
}

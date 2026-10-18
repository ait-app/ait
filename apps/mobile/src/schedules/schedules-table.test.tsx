// @vitest-environment jsdom
import React from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ScheduleTarget } from "@ait/protocol/schedule/types";
import type { ScheduleRowView } from "@/components/schedules/schedules-table";

const mutations = vi.hoisted(() => ({
  pauseSchedule: vi.fn(),
  resumeSchedule: vi.fn(),
  runScheduleNow: vi.fn(),
  deleteSchedule: vi.fn(),
}));
vi.mock("@/hooks/use-schedule-mutations", () => ({
  useScheduleMutations: () => mutations,
}));
vi.mock("@/components/provider-icons", () => ({ getProviderIcon: () => () => null }));

vi.stubGlobal("React", React);
const { SchedulesTable } = await import("@/components/schedules/schedules-table");

const agentTarget: ScheduleTarget = {
  type: "agent",
  agentId: "11111111-1111-4111-8111-111111111111",
};
const newAgentTarget: ScheduleTarget = { type: "new-agent", config: { provider: "codex" } };

function task(target: ScheduleTarget = agentTarget): ScheduleRowView {
  return {
    schedule: {
      id: "daily-release",
      serverId: "host-a",
      serverName: "My host",
      workspaceId: "workspace-a",
      name: "Daily release",
      prompt: "Publish a release",
      target,
      cadence: { type: "every", everyMs: 86_400_000 },
      status: "active",
      createdAt: "2026-01-01T00:00:00Z",
      updatedAt: "2026-01-01T00:00:00Z",
      nextRunAt: null,
      lastRunAt: null,
      pausedAt: null,
      maxRuns: null,
      expiresAt: null,
    },
    targetLabel: "Release agent",
    provider: null,
    state: "active",
    serverName: "My host",
    singleHost: true,
  };
}

function paused(row: ScheduleRowView): ScheduleRowView {
  return { ...row, state: "paused", schedule: { ...row.schedule, status: "paused" } };
}

beforeEach(() => {
  vi.stubGlobal("React", React);
  vi.clearAllMocks();
  mutations.pauseSchedule.mockResolvedValue(undefined);
  mutations.resumeSchedule.mockResolvedValue(undefined);
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

it.each([agentTarget, newAgentTarget])(
  "pauses and resumes a $type task directly in the list without editing it",
  async (target) => {
    const row = task(target);
    const onEdit = vi.fn();
    const view = render(<SchedulesTable rows={[row]} onEditSchedule={onEdit} />);

    fireEvent.click(screen.getByTestId("schedule-toggle-status-daily-release"));
    await waitFor(() => expect(mutations.pauseSchedule).toHaveBeenCalledWith("daily-release"));
    await waitFor(() =>
      expect(
        screen.getByTestId("schedule-toggle-status-daily-release").getAttribute("aria-busy"),
      ).not.toBe("true"),
    );

    view.rerender(<SchedulesTable rows={[paused(row)]} onEditSchedule={onEdit} />);
    const resume = screen.getByTestId("schedule-toggle-status-daily-release");
    expect(resume.textContent).toContain("Resume");
    fireEvent.click(resume);
    await waitFor(() => expect(mutations.resumeSchedule).toHaveBeenCalledWith("daily-release"));
    expect(onEdit).not.toHaveBeenCalled();

    fireEvent.click(screen.getByTestId("schedule-row-daily-release"));
    expect(onEdit).toHaveBeenCalledWith(paused(row).schedule);
  },
);

it.each([false, true])(
  "offers the existing Agent task's status action in its menu (paused=%s)",
  async (isPaused) => {
    const row = isPaused ? paused(task()) : task();
    const onEdit = vi.fn();
    render(<SchedulesTable rows={[row]} onEditSchedule={onEdit} />);
    fireEvent.click(screen.getByTestId("schedule-kebab-daily-release"));
    const action = await screen.findByTestId(
      `schedule-menu-${isPaused ? "resume" : "pause"}-daily-release`,
    );
    expect(action.getAttribute("aria-disabled")).not.toBe("true");
    fireEvent.click(action);
    await waitFor(() =>
      expect(isPaused ? mutations.resumeSchedule : mutations.pauseSchedule).toHaveBeenCalledWith(
        "daily-release",
      ),
    );
    expect(onEdit).not.toHaveBeenCalled();
  },
);

it("blocks a reverse action while an optimistic pause is still pending", async () => {
  const pending = Promise.withResolvers<void>();
  mutations.pauseSchedule.mockReturnValue(pending.promise);
  const row = task();
  const view = render(<SchedulesTable rows={[row]} onEditSchedule={vi.fn()} />);
  fireEvent.click(screen.getByTestId("schedule-toggle-status-daily-release"));
  view.rerender(<SchedulesTable rows={[paused(row)]} onEditSchedule={vi.fn()} />);

  const button = screen.getByTestId("schedule-toggle-status-daily-release");
  expect(button.getAttribute("aria-disabled")).toBe("true");
  fireEvent.click(button);
  fireEvent.click(screen.getByTestId("schedule-kebab-daily-release"));
  const action = await screen.findByTestId("schedule-menu-resume-daily-release");
  expect(action.getAttribute("aria-disabled")).toBe("true");
  fireEvent.click(action);
  expect(mutations.resumeSchedule).not.toHaveBeenCalled();

  await act(async () => pending.resolve());
  await waitFor(() => expect(button.getAttribute("aria-disabled")).not.toBe("true"));
});

it("allows retrying a failed pause", async () => {
  mutations.pauseSchedule.mockRejectedValueOnce(new Error("Host unavailable"));
  render(<SchedulesTable rows={[task()]} onEditSchedule={vi.fn()} />);
  const button = screen.getByTestId("schedule-toggle-status-daily-release");
  fireEvent.click(button);
  await waitFor(() => expect(mutations.pauseSchedule).toHaveBeenCalledTimes(1));
  await waitFor(() => expect(button.getAttribute("aria-disabled")).not.toBe("true"));
  fireEvent.click(button);
  await waitFor(() => expect(mutations.pauseSchedule).toHaveBeenCalledTimes(2));
});

it.each(["expired", "finished", "targetGone"] as const)(
  "does not offer a direct status action for a %s task",
  (state) => {
    render(<SchedulesTable rows={[{ ...task(), state }]} onEditSchedule={vi.fn()} />);
    expect(screen.queryByTestId("schedule-toggle-status-daily-release")).toBeNull();
  },
);

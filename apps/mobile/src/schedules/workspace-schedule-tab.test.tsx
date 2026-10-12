// @vitest-environment jsdom
import React from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { StoredSchedule } from "@ait/protocol/schedule/types";
import { WorkspaceScheduleTab } from "@/components/schedules/workspace-schedule-tab";

const mocks = vi.hoisted(() => ({
  run: vi.fn(),
  pause: vi.fn(),
  list: [] as StoredSchedule[],
  detail: {
    data: null as StoredSchedule | null,
    isPending: false,
    isError: false,
    refetch: vi.fn(),
  },
}));
vi.mock("@/schedules/use-workspace-schedules", () => ({
  useScheduleDetail: () => mocks.detail,
  useWorkspaceSchedules: () => ({ data: mocks.list, isPending: false, isError: false }),
}));
vi.mock("@/hooks/use-schedule-mutations", () => ({
  useScheduleMutations: () => ({ runScheduleNow: mocks.run, pauseSchedule: mocks.pause }),
}));
vi.mock("@/hooks/use-aggregated-agents", () => ({ useAggregatedAgents: () => ({ agents: [] }) }));
vi.mock("@/components/schedules/schedule-form-sheet", () => ({
  WorkspaceScheduleForm: () => null,
}));

const task: StoredSchedule = {
  id: "task-a",
  workspaceId: "workspace-a",
  name: "Review",
  prompt: "Review changes",
  target: { type: "new-agent", config: { provider: "codex" } },
  cadence: { type: "every", everyMs: 3_600_000 },
  status: "active",
  runs: [],
  createdAt: "2026-01-01T00:00:00Z",
  updatedAt: "2026-01-01T00:00:00Z",
  nextRunAt: null,
  lastRunAt: null,
  pausedAt: null,
  maxRuns: null,
  expiresAt: null,
};
const clients: QueryClient[] = [];
function open(list = false) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  clients.push(client);
  return render(
    <QueryClientProvider client={client}>
      <WorkspaceScheduleTab
        serverId="host-a"
        workspace={{ id: "workspace-a", cwd: "/repo", name: "Feature" }}
        scheduleId={list ? undefined : "task-a"}
        interactive
        onBrowse={vi.fn()}
        onOpen={vi.fn()}
        onEditTask={vi.fn()}
        onCreate={vi.fn()}
        onOpenAgent={vi.fn()}
      />
    </QueryClientProvider>,
  );
}
beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal("React", React);
  mocks.detail.data = task;
  mocks.list = [];
  mocks.run.mockResolvedValue(undefined);
  mocks.pause.mockResolvedValue(undefined);
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  clients.splice(0).forEach((client) => client.clear());
});

it("prevents displaying or running a task from another workspace", () => {
  mocks.detail.data = { ...task, workspaceId: "workspace-b" };
  open();
  expect(screen.queryByTestId("schedule-run-now")).toBeNull();
  expect(screen.queryByTestId("workspace-schedule-details")).toBeNull();
});
it("allows pausing while a manual run is pending and prevents another run", async () => {
  let finish: (() => void) | undefined;
  mocks.run.mockImplementation(
    () =>
      new Promise<void>((resolve) => {
        finish = resolve;
      }),
  );
  open();
  fireEvent.click(screen.getByTestId("schedule-run-now"));
  await waitFor(() => expect(mocks.run).toHaveBeenCalledWith("task-a"));
  expect((screen.getByTestId("schedule-run-now") as HTMLButtonElement).disabled).toBe(true);
  expect((screen.getByTestId("schedule-toggle-status") as HTMLButtonElement).disabled).toBe(false);
  fireEvent.click(screen.getByTestId("schedule-toggle-status"));
  await waitFor(() => expect(mocks.pause).toHaveBeenCalledWith("task-a"));
  finish?.();
  await waitFor(() =>
    expect((screen.getByTestId("schedule-run-now") as HTMLButtonElement).disabled).toBe(false),
  );
});
it("shows failed actions without discarding task details", async () => {
  mocks.pause.mockRejectedValue(new Error("Host unavailable"));
  open();
  fireEvent.click(screen.getByTestId("schedule-toggle-status"));
  await waitFor(() => expect(screen.getByText("Host unavailable")).toBeTruthy());
  expect(screen.getByTestId("workspace-schedule-details")).toBeTruthy();
});

it("opens the time list by default and excludes other workspaces", () => {
  const nextRunAt = new Date(Date.now() + 3_600_000).toISOString();
  mocks.list = [
    { ...task, nextRunAt },
    { ...task, id: "foreign", workspaceId: "workspace-b", nextRunAt },
  ];
  open(true);
  expect(screen.getByTestId("workspace-schedules-list")).toBeTruthy();
  expect(screen.getByTestId("workspace-schedule-task-a")).toBeTruthy();
  expect(screen.queryByTestId("workspace-schedule-foreign")).toBeNull();
});

it("displays a legacy single trigger as one run rather than unlimited", () => {
  const at = new Date(Date.now() + 3_600_000).toISOString();
  mocks.list = [{ ...task, cadence: { type: "once", at }, nextRunAt: at, maxRuns: null }];
  open(true);
  expect(screen.getByTestId("workspace-schedule-task-a").textContent).toContain("1 run");
});

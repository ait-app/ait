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
  WorkspaceScheduleForm: () => <div data-testid="schedule-editor" />,
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
function open(
  list = false,
  overrides: Partial<React.ComponentProps<typeof WorkspaceScheduleTab>> = {},
) {
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
        {...overrides}
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

it("restores a guest edit tab as readable details without task controls", () => {
  open(false, { serverId: "shared:grant", view: "edit" });
  expect(screen.getByTestId("workspace-schedule-details")).toBeTruthy();
  expect(screen.getByText("Review changes")).toBeTruthy();
  for (const id of [
    "schedule-editor",
    "schedule-run-now",
    "schedule-toggle-status",
    "schedule-edit",
    "schedule-delete",
    "workspace-schedules-new",
  ]) {
    expect(screen.queryByTestId(id)).toBeNull();
  }
  expect(mocks.run).not.toHaveBeenCalled();
  expect(mocks.pause).not.toHaveBeenCalled();
});

it("restores a guest create tab as a list whose rows open readable details", () => {
  const onOpen = vi.fn();
  const onEditTask = vi.fn();
  mocks.list = [{ ...task, nextRunAt: new Date(Date.now() + 3_600_000).toISOString() }];
  open(true, { serverId: "shared:grant", view: "create", onOpen, onEditTask });
  expect(screen.getByTestId("workspace-schedules-list")).toBeTruthy();
  expect(screen.queryByTestId("schedule-editor")).toBeNull();
  expect(screen.queryByTestId("workspace-schedules-new")).toBeNull();
  fireEvent.click(screen.getByTestId("workspace-schedule-task-a"));
  expect(onOpen).toHaveBeenCalledWith("task-a");
  expect(onEditTask).not.toHaveBeenCalled();
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

function upcomingTasks(count: number): StoredSchedule[] {
  const nextRunAt = new Date(Date.now() + 3_600_000).toISOString();
  return Array.from({ length: count }, (_, index) => ({
    ...task,
    id: `task-${String(index).padStart(2, "0")}`,
    nextRunAt,
  }));
}

function followsInDocument(earlier: Element, later: Element): boolean {
  return Boolean(earlier.compareDocumentPosition(later) & Node.DOCUMENT_POSITION_FOLLOWING);
}

it("has no header and ends the list with the only new button", () => {
  mocks.list = upcomingTasks(1);
  open(true);
  expect(screen.queryByText("Workspace schedules")).toBeNull();
  expect(screen.queryByText("Feature")).toBeNull();
  expect(screen.getAllByTestId("workspace-schedules-new")).toHaveLength(1);
  expect(
    followsInDocument(
      screen.getByTestId("workspace-schedule-task-00"),
      screen.getByTestId("workspace-schedules-new"),
    ),
  ).toBe(true);
});

it("offers the new button on an empty filter and not in task details", () => {
  open(true, { filter: "paused" });
  expect(screen.getAllByTestId("workspace-schedules-new")).toHaveLength(1);
  cleanup();
  open();
  expect(screen.queryByTestId("workspace-schedules-new")).toBeNull();
  expect(screen.getByTestId("workspace-schedules-browse")).toBeTruthy();
});

it("shows 20 tasks per page with the new button on every page", () => {
  mocks.list = upcomingTasks(21);
  open(true);
  expect(screen.getByTestId("workspace-schedule-task-19")).toBeTruthy();
  expect(screen.queryByTestId("workspace-schedule-task-20")).toBeNull();
  expect(screen.getByTestId("workspace-schedules-page-status").textContent).toBe("Page 1 of 2");

  fireEvent.click(screen.getByTestId("workspace-schedules-page-next"));

  expect(screen.getByTestId("workspace-schedule-task-20")).toBeTruthy();
  expect(screen.queryByTestId("workspace-schedule-task-19")).toBeNull();
  expect(screen.getByTestId("workspace-schedules-new")).toBeTruthy();
  expect((screen.getByTestId("workspace-schedules-page-next") as HTMLButtonElement).disabled).toBe(
    true,
  );
});

it("hides the pager when one page holds every task", () => {
  mocks.list = upcomingTasks(20);
  open(true);
  expect(screen.queryByTestId("workspace-schedules-page-status")).toBeNull();
});

it("starts over at page one after visiting another filter and back", () => {
  mocks.list = upcomingTasks(21);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  clients.push(client);
  const tab = (filter?: "paused") => (
    <QueryClientProvider client={client}>
      <WorkspaceScheduleTab
        serverId="host-a"
        workspace={{ id: "workspace-a", cwd: "/repo", name: "Feature" }}
        filter={filter}
        interactive
        onBrowse={vi.fn()}
        onOpen={vi.fn()}
        onEditTask={vi.fn()}
        onCreate={vi.fn()}
        onOpenAgent={vi.fn()}
      />
    </QueryClientProvider>
  );
  const { rerender } = render(tab());
  fireEvent.click(screen.getByTestId("workspace-schedules-page-next"));
  expect(screen.getByTestId("workspace-schedules-page-status").textContent).toBe("Page 2 of 2");

  rerender(tab("paused"));
  rerender(tab());

  expect(screen.getByTestId("workspace-schedules-page-status").textContent).toBe("Page 1 of 2");
});

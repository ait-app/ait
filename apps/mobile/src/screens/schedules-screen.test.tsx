// @vitest-environment jsdom
import React from "react";
import { cleanup, fireEvent, render, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AggregatedSchedule, UseSchedulesResult } from "@/hooks/use-schedules";
import { i18n } from "@/i18n/i18next";
import { ALL_HOSTS_OPTION_ID } from "@/components/hosts/host-picker-constants";
import { SchedulesScreen } from "./schedules-screen";

const state = vi.hoisted(() => ({
  result: {
    loadState: { status: "loaded", data: [] },
    hostErrors: [],
    isError: false,
    error: null,
    refetch: vi.fn(),
    isRefetching: false,
  } as UseSchedulesResult,
  hosts: [
    { serverId: "host-a", label: "Host A" },
    { serverId: "host-b", label: "Host B" },
  ],
  agents: [],
  projects: [],
  workspaces: [],
  runtime: {
    subscribeAll: () => () => {},
    getVersion: () => 0,
    getSnapshot: () => ({ hasEverLoadedAgentDirectory: true }),
  },
}));

vi.mock("@react-navigation/native", () => ({ useIsFocused: () => true }));
vi.mock("@/hooks/use-schedules", () => ({ useSchedules: () => state.result }));
vi.mock("@/hooks/use-aggregated-agents", () => ({
  useAggregatedAgents: () => ({ agents: state.agents }),
}));
vi.mock("@/hooks/use-projects", () => ({ useProjects: () => ({ projects: state.projects }) }));
vi.mock("@/schedules/use-schedule-workspaces", () => ({
  useScheduleWorkspaces: () => state.workspaces,
}));
vi.mock("@/runtime/host-runtime", () => ({
  useHosts: () => state.hosts,
  getHostRuntimeStore: () => state.runtime,
}));
vi.mock("@/components/headers/menu-header", () => ({ MenuHeader: () => null }));
vi.mock("@/components/schedules/schedule-form-sheet", () => ({ ScheduleFormSheet: () => null }));
vi.mock("@/components/provider-icons", () => ({ getProviderIcon: () => () => null }));
vi.mock("@/hooks/use-schedule-mutations", () => ({ useScheduleMutations: () => ({}) }));
vi.mock("@/components/hosts/host-filter", () => ({
  HostFilter: ({
    selectedHost,
    onSelectHost,
  }: {
    selectedHost: string;
    onSelectHost: (serverId: string) => void;
  }) => (
    <select aria-label="Host" value={selectedHost} onChange={(e) => onSelectHost(e.target.value)}>
      <option value={ALL_HOSTS_OPTION_ID}>All hosts</option>
      <option value="host-a">Host A</option>
      <option value="host-b">Host B</option>
    </select>
  ),
}));

function schedule(index: number, overrides: Partial<AggregatedSchedule> = {}): AggregatedSchedule {
  const finishedAt = new Date(Date.UTC(2026, 0, 1, 0, index)).toISOString();
  return {
    id: `ended-${index}`,
    serverId: index % 2 === 0 ? "host-a" : "host-b",
    serverName: index % 2 === 0 ? "Host A" : "Host B",
    workspaceId: "workspace",
    name: `Schedule ${index}`,
    prompt: "Review the project",
    target: { type: "new-agent", config: { provider: "codex", cwd: "/project" } },
    cadence: { type: "every", everyMs: 60_000 },
    status: "completed",
    createdAt: new Date(Date.UTC(2025, 11, 31, 0, 60 - index)).toISOString(),
    updatedAt: finishedAt,
    lastRunAt: finishedAt,
    nextRunAt: null,
    pausedAt: null,
    maxRuns: null,
    expiresAt: null,
    ...overrides,
  };
}

beforeEach(async () => {
  state.result.loadState = { status: "loaded", data: [] };
  state.result.hostErrors = [];
  state.result.isError = false;
  await i18n.changeLanguage("en");
});
afterEach(cleanup);

function rowIds(section: HTMLElement): string[] {
  return within(section)
    .queryAllByTestId(/^schedule-row-/)
    .map((row) => row.dataset.testid!);
}

describe("SchedulesScreen sections", () => {
  it("shows all unfinished schedules first and pages ended schedules by completion time", () => {
    state.result.loadState = {
      status: "loaded",
      data: [
        ...Array.from({ length: 12 }, (_, index) =>
          schedule(index, { id: `active-${index}`, status: index === 0 ? "paused" : "active" }),
        ),
        ...Array.from({ length: 23 }, (_, index) => schedule(index)),
      ],
    };
    const view = render(<SchedulesScreen />);
    const active = view.getByTestId("schedules-active-section");
    const ended = view.getByTestId("schedules-ended-section");
    expect(active.compareDocumentPosition(ended) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(rowIds(active)).toHaveLength(12);
    expect(within(active).getByText("Active (12)")).toBeTruthy();
    expect(within(ended).getByText("Ended (23)")).toBeTruthy();
    expect(view.queryByTestId("schedules-status-filter")).toBeNull();
    expect(rowIds(ended)).toEqual(
      Array.from({ length: 10 }, (_, index) => `schedule-row-ended-${22 - index}`),
    );

    fireEvent.click(within(ended).getByRole("button", { name: "Show more" }));
    expect(rowIds(ended)).toHaveLength(20);
    fireEvent.click(within(ended).getByRole("button", { name: "Show more" }));
    expect(rowIds(ended)).toEqual(
      Array.from({ length: 23 }, (_, index) => `schedule-row-ended-${22 - index}`),
    );
    expect(within(ended).queryByRole("button", { name: "Show more" })).toBeNull();
    expect(rowIds(active)).toHaveLength(12);
  });

  it("resets ended pagination on host changes and sorts within the selected host", () => {
    state.result.loadState = {
      status: "loaded",
      data: Array.from({ length: 23 }, (_, index) => schedule(index)),
    };
    const view = render(<SchedulesScreen />);
    fireEvent.click(view.getByRole("button", { name: "Show more" }));
    fireEvent.change(view.getByRole("combobox", { name: "Host" }), { target: { value: "host-a" } });
    const ended = view.getByTestId("schedules-ended-section");
    expect(within(ended).getByText("Ended (12)")).toBeTruthy();
    expect(rowIds(ended)).toEqual(
      Array.from({ length: 10 }, (_, index) => `schedule-row-ended-${22 - index * 2}`),
    );
    fireEvent.change(view.getByRole("combobox", { name: "Host" }), {
      target: { value: ALL_HOSTS_OPTION_ID },
    });
    expect(rowIds(view.getByTestId("schedules-ended-section"))).toHaveLength(10);
  });

  it.each([0, 10])(
    "does not offer more for %i ended schedules and keeps both sections",
    (count) => {
      state.result.loadState = {
        status: "loaded",
        data: Array.from({ length: count }, (_, index) => schedule(index)),
      };
      const view = render(<SchedulesScreen />);
      expect(view.getByTestId("schedules-active-section")).toBeTruthy();
      expect(rowIds(view.getByTestId("schedules-ended-section"))).toHaveLength(count);
      expect(view.queryByRole("button", { name: "Show more" })).toBeNull();
      expect(view.getByTestId("schedules-empty-new")).toBeTruthy();
    },
  );

  it("shows empty sections without a loading error when every host fails", () => {
    state.result.loadState = { status: "loading" };
    state.result.isError = true;
    const view = render(<SchedulesScreen />);
    expect(view.getByTestId("schedules-active-section")).toBeTruthy();
    expect(view.getByTestId("schedules-ended-section")).toBeTruthy();
    expect(view.queryByTestId("schedules-retry")).toBeNull();
    expect(view.queryByText("Unable to load schedules")).toBeNull();
  });

  it("keeps available schedules without a partial-host error banner", () => {
    state.result.loadState = { status: "loaded", data: [schedule(1)] };
    state.result.hostErrors = [{ serverId: "host-a", serverName: "Host A", message: "forbidden" }];
    const view = render(<SchedulesScreen />);
    expect(view.getByTestId("schedule-row-ended-1")).toBeTruthy();
    expect(view.queryByTestId("schedules-host-errors")).toBeNull();
  });
});

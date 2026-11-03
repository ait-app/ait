// @vitest-environment jsdom
import React, { useEffect } from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { create } from "zustand";
import type { ScheduleSummary } from "@ait/protocol/schedule/types";
import {
  ScheduleFormSheet,
  WorkspaceScheduleForm,
} from "@/components/schedules/schedule-form-sheet";
import type { ScheduleFormModel } from "./schedule-form-model";
import type { ProjectSummary } from "@/utils/projects";
import { i18n } from "@/i18n/i18next";
import type { HostChoice } from "@/hosts/host-groups";
import { createAccountRelayHostConnection } from "@/types/host-connection";

const { createSchedule, updateSchedule, updatePreferences } = vi.hoisted(() => ({
  createSchedule: vi.fn(async () => ({ id: "schedule-created" })),
  updateSchedule: vi.fn(async () => {}),
  updatePreferences: vi.fn(async () => {}),
}));
const hosts: HostChoice[] = [
  { serverId: "host-a", label: "Host A" },
  { serverId: "host-b", label: "Host B" },
];
const projects: ProjectSummary[] = [];
const scopeWorkspaces = [
  {
    id: "workspace-a",
    projectId: "project-a",
    serverId: "host-a",
    serverName: "Host A",
    name: "Feature",
    cwd: "/worktrees/feature",
  },
  {
    id: "workspace-b",
    projectId: "project-b",
    serverId: "host-a",
    serverName: "Host A",
    name: "Other project workspace",
    cwd: "/worktrees/feature",
  },
  {
    id: "workspace-a",
    projectId: "project-a",
    serverId: "host-b",
    serverName: "Host B",
    name: "Other host workspace",
    cwd: "/remote/feature",
  },
];
const agentId = "11111111-1111-4111-8111-111111111111";
const agents = [
  {
    id: agentId,
    serverId: "host-a",
    workspaceId: "workspace-a",
    title: "My agent",
    archivedAt: null,
  },
  {
    id: "foreign",
    serverId: "host-a",
    workspaceId: "workspace-b",
    title: "Foreign agent",
    archivedAt: null,
  },
  {
    id: "archived",
    serverId: "host-a",
    workspaceId: "workspace-a",
    title: "Archived agent",
    archivedAt: "now",
  },
  {
    id: "other-host",
    serverId: "host-b",
    workspaceId: "workspace-a",
    title: "Other host agent",
    archivedAt: null,
  },
];
const preferences = {};

vi.mock("@/hooks/use-projects", () => ({ useProjects: () => ({ projects }) }));
vi.mock("@/schedules/use-schedule-workspaces", () => ({
  useScheduleWorkspaces: () => scopeWorkspaces,
}));
vi.mock("@/hooks/use-aggregated-agents", () => ({
  useAggregatedAgents: () => ({ agents }),
}));
vi.mock("@/runtime/host-runtime", () => ({ useHosts: () => hosts }));
vi.mock("@/stores/session-store", () => ({
  useSessionStore: create(() => ({ sessions: {} })),
}));
vi.mock("@/hooks/use-form-preferences", () => ({
  useFormPreferences: () => ({ preferences, updatePreferences }),
  mergeProviderPreferences: ({ preferences: current }: { preferences: object }) => current,
}));
vi.mock("@/hooks/use-schedule-mutations", () => ({
  useScheduleMutations: () => ({
    createSchedule,
    updateSchedule,
    isCreating: false,
    isUpdating: false,
  }),
}));
vi.mock("@/constants/layout", () => ({ useIsCompactFormFactor: () => false }));
vi.mock("@/schedules/use-schedule-form-provider-snapshot", () => ({
  useScheduleFormProviderSnapshot: (model: ScheduleFormModel) => {
    useEffect(() => {
      model.applyProviderSnapshot("host-a", {
        entries: [
          {
            provider: "mock",
            label: "Mock",
            status: "ready",
            enabled: true,
            fetchedAt: "2026-10-12T00:00:00Z",
            models: [{ provider: "mock", id: "model-a", label: "Model A" }],
          },
        ],
      });
    }, [model]);
    return { isLoading: false, isFetching: false, isRefreshing: false };
  },
}));
vi.mock("@/components/combined-model-selector", () => ({
  CombinedModelSelector: ({
    onSelect,
  }: {
    onSelect: (provider: string, model: string) => void;
  }) => <button onClick={() => onSelect("mock", "model-a")}>Choose model</button>,
}));
vi.mock("@/components/schedules/cadence-editor", () => ({
  CadenceEditor: () => <div>Cadence</div>,
}));
vi.mock("@/components/adaptive-modal-sheet", () => ({
  AdaptiveModalSheet: ({ children, testID }: { children: React.ReactNode; testID?: string }) => (
    <div data-testid={testID}>{children}</div>
  ),
}));
vi.mock("@/components/ui/switch", () => ({ Switch: () => null }));
vi.mock("@/components/ui/combobox", () => ({ ComboboxItem: () => null }));
vi.mock("@/components/ui/select-field", () => ({
  SelectField: ({
    label,
    triggerTestID,
    options,
    value,
    onChange,
    disabled,
  }: {
    label: string;
    triggerTestID?: string;
    options: { id: string; value: string; label: string }[];
    value: string;
    onChange: (value: string) => void;
    disabled?: boolean;
  }) => (
    <select
      aria-label={label}
      data-testid={triggerTestID}
      value={value ?? ""}
      disabled={disabled}
      onChange={(event) => onChange(event.target.value)}
    >
      <option value="">Choose</option>
      {options.map((option) => (
        <option key={option.id} value={option.value}>
          {option.label}
        </option>
      ))}
    </select>
  ),
  SelectFieldTrigger: () => null,
}));
vi.mock("@/components/hosts/host-picker", () => ({ HostStatusDotSlot: () => null }));
vi.mock("@/components/provider-icons", () => ({ getProviderIcon: () => () => null }));
vi.mock("@/components/ui/form-field", () => ({
  Field: ({ label, children }: { label: string; children: React.ReactNode }) => (
    <div>
      <span>{label}</span>
      {children}
    </div>
  ),
  FormTextInput: ({
    accessibilityLabel,
    initialValue,
    onChangeText,
  }: {
    accessibilityLabel: string;
    initialValue: string;
    onChangeText: (value: string) => void;
  }) => (
    <input
      aria-label={accessibilityLabel}
      defaultValue={initialValue}
      onChange={(event) => onChangeText(event.target.value)}
    />
  ),
}));

describe("workspace schedule form", () => {
  it("offers one Host when multiple resources were shared from that Host", () => {
    const original = [...hosts];
    const connection = createAccountRelayHostConnection({
      center: "https://center.test/api",
      hostId: "11111111-1111-4111-8111-111111111111",
      shareId: "22222222-2222-4222-8222-222222222222",
      nodeId: "node",
      shareResourceKind: "workspace",
    });
    hosts.splice(
      0,
      hosts.length,
      { serverId: "share-workspace", label: "Shared Host", connections: [connection] },
      {
        serverId: "share-project",
        label: "Shared Host",
        connections: [
          {
            ...connection,
            shareId: "33333333-3333-4333-8333-333333333333",
            shareResourceKind: "project",
          },
        ],
      },
    );
    try {
      render(
        <ScheduleFormSheet visible mode="create" serverId="share-workspace" onClose={() => {}} />,
      );
      expect(screen.getAllByRole("option", { name: "Shared Host" })).toHaveLength(1);
      expect(screen.getByTestId("schedule-host-trigger")).toHaveProperty("value", "share-project");
    } finally {
      hosts.splice(0, hosts.length, ...original);
    }
  });
  beforeEach(() => {
    vi.stubGlobal("React", React);
  });

  afterEach(async () => {
    cleanup();
    vi.clearAllMocks();
    vi.unstubAllGlobals();
    projects.length = 0;
    await i18n.changeLanguage("en");
  });

  it("submits from the inline workspace form using its directory without a project selection", async () => {
    const onClose = vi.fn();
    const { container } = render(
      <WorkspaceScheduleForm
        serverId="host-a"
        workspace={{ id: "workspace-a", name: "Feature branch", cwd: "/worktrees/feature" }}
        onClose={onClose}
      />,
    );

    expect(screen.getByText("Feature branch")).toBeTruthy();
    expect(screen.getByText("/worktrees/feature")).toBeTruthy();
    expect(container.querySelector('[data-testid="schedule-form-sheet"]')).toBeNull();
    expect(container.querySelector('[data-testid="schedule-host-trigger"]')).toBeNull();
    expect(container.querySelector('[data-testid="schedule-project-trigger"]')).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Choose model" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Prompt" }), {
      target: { value: "Check this workspace" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Create schedule" }));

    await waitFor(() => expect(createSchedule).toHaveBeenCalledOnce());
    expect(createSchedule).toHaveBeenCalledWith(
      expect.objectContaining({
        prompt: "Check this workspace",
        workspaceId: "workspace-a",
        runOnCreate: false,
        maxRuns: 1,
        cadence: { type: "every", everyMs: 3_600_000 },
        target: {
          type: "new-agent",
          config: expect.objectContaining({
            provider: "mock",
            model: "model-a",
            cwd: "/worktrees/feature",
          }),
        },
      }),
    );
    expect(onClose).toHaveBeenCalledOnce();
  });
  it("creates a schedule for an active agent in the owning workspace", async () => {
    render(
      <WorkspaceScheduleForm
        serverId="host-a"
        workspace={{ id: "workspace-a", name: "Feature", cwd: "/worktrees/feature" }}
        onClose={vi.fn()}
      />,
    );
    const selector = screen.getByLabelText("Execution agent");
    expect(screen.queryByText("Foreign agent")).toBeNull();
    expect(screen.queryByText("Archived agent")).toBeNull();
    expect(screen.queryByText("Other host agent")).toBeNull();
    expect(screen.queryByTestId("schedule-target-kind-trigger")).toBeNull();
    expect(screen.queryByTestId("schedule-agent-trigger")).toBeNull();
    fireEvent.change(selector, { target: { value: agentId } });
    expect(screen.queryByRole("button", { name: "Choose model" })).toBeNull();
    fireEvent.change(screen.getByRole("textbox", { name: "Prompt" }), {
      target: { value: "Review the changes" },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "Max runs" }), {
      target: { value: "2.5" },
    });
    expect(
      (screen.getByRole("button", { name: "Create schedule" }) as HTMLButtonElement).disabled,
    ).toBe(true);
    fireEvent.change(screen.getByRole("textbox", { name: "Max runs" }), { target: { value: "2" } });
    fireEvent.click(screen.getByRole("button", { name: "Create schedule" }));
    await waitFor(() => expect(createSchedule).toHaveBeenCalledOnce());
    expect(createSchedule).toHaveBeenCalledWith(
      expect.objectContaining({
        workspaceId: "workspace-a",
        prompt: "Review the changes",
        target: { type: "agent", agentId },
        maxRuns: 2,
      }),
    );
    expect(updatePreferences).not.toHaveBeenCalled();
  });

  it("keeps the prompt and new-agent configuration when switching the unified selector", async () => {
    render(
      <WorkspaceScheduleForm
        serverId="host-a"
        workspace={{ id: "workspace-a", name: "Feature", cwd: "/worktrees/feature" }}
        onClose={vi.fn()}
      />,
    );
    const selector = screen.getByLabelText("Execution agent") as HTMLSelectElement;
    expect(selector.value).toBe("new-agent");
    fireEvent.click(screen.getByRole("button", { name: "Choose model" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Prompt" }), {
      target: { value: "Keep my review instructions" },
    });
    fireEvent.change(selector, { target: { value: agentId } });
    expect(screen.queryByRole("button", { name: "Choose model" })).toBeNull();
    fireEvent.change(selector, { target: { value: "new-agent" } });
    expect(screen.getByRole("button", { name: "Choose model" })).toBeTruthy();
    expect((screen.getByRole("textbox", { name: "Prompt" }) as HTMLInputElement).value).toBe(
      "Keep my review instructions",
    );
    fireEvent.click(screen.getByRole("button", { name: "Create schedule" }));
    await waitFor(() =>
      expect(createSchedule).toHaveBeenCalledWith(
        expect.objectContaining({
          prompt: "Keep my review instructions",
          target: {
            type: "new-agent",
            config: expect.objectContaining({ provider: "mock", model: "model-a" }),
          },
        }),
      ),
    );
  });

  it("keeps the created task in the workspace tab through the saved callback", async () => {
    const onSaved = vi.fn();
    const onClose = vi.fn();
    render(
      <WorkspaceScheduleForm
        serverId="host-a"
        workspace={{ id: "workspace-a", name: "Feature", cwd: "/worktrees/feature" }}
        onClose={onClose}
        onSaved={onSaved}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Choose model" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Prompt" }), {
      target: { value: "Check changes" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Create schedule" }));
    await waitFor(() => expect(onSaved).toHaveBeenCalledWith({ id: "schedule-created" }));
    expect(onClose).not.toHaveBeenCalled();
  });

  it("rejects malformed or past timing fields before submitting", async () => {
    render(
      <WorkspaceScheduleForm
        serverId="host-a"
        workspace={{ id: "workspace-a", name: "Feature", cwd: "/worktrees/feature" }}
        onClose={vi.fn()}
      />,
    );
    fireEvent.change(screen.getByLabelText("Execution agent"), { target: { value: agentId } });
    fireEvent.change(screen.getByRole("textbox", { name: "Prompt" }), {
      target: { value: "Review changes" },
    });
    const submit = screen.getByRole("button", { name: "Create schedule" }) as HTMLButtonElement;
    const start = screen.getByRole("textbox", { name: "First trigger (optional)" });
    const stop = screen.getByRole("textbox", { name: "Stop after (optional)" });
    fireEvent.change(start, { target: { value: "2000-01-01 09:00" } });
    expect(submit.disabled).toBe(true);
    fireEvent.change(start, { target: { value: "2035-01-01 09:00" } });
    fireEvent.change(stop, { target: { value: "2035-02-30 09:00" } });
    expect(submit.disabled).toBe(true);
    fireEvent.change(stop, { target: { value: "2035-01-01 08:00" } });
    expect(submit.disabled).toBe(true);
    fireEvent.change(stop, { target: { value: "2035-01-02 09:00" } });
    expect(submit.disabled).toBe(false);
    fireEvent.click(submit);
    await waitFor(() => expect(createSchedule).toHaveBeenCalledOnce());
    expect(createSchedule).toHaveBeenCalledWith(
      expect.objectContaining({ startAt: expect.any(String), expiresAt: expect.any(String) }),
    );
  });
  it("keeps unlimited editable and sends no limit when a new task clears the default", async () => {
    render(
      <WorkspaceScheduleForm
        serverId="host-a"
        workspace={{ id: "workspace-a", name: "Feature", cwd: "/worktrees/feature" }}
        onClose={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Choose model" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Prompt" }), {
      target: { value: "Repeat checks" },
    });
    expect((screen.getByRole("textbox", { name: "Max runs" }) as HTMLInputElement).value).toBe("1");
    fireEvent.change(screen.getByRole("textbox", { name: "Max runs" }), { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "Create schedule" }));
    await waitFor(() =>
      expect(createSchedule).toHaveBeenCalledWith(expect.objectContaining({ maxRuns: undefined })),
    );
  });
  it.each([
    {
      language: "en",
      code: "schedule_running",
      expected: "This schedule is running. Wait for the current run to finish before editing it.",
    },
    {
      language: "zh-CN",
      code: "schedule_running",
      expected: "计划正在运行，请等待本次运行结束后再修改。",
    },
    {
      language: "en",
      code: "schedule_request_failed",
      expected: "Unable to save schedule: Storage unavailable",
    },
  ])("explains $code when saving in $language", async ({ language, code, expected }) => {
    await i18n.changeLanguage(language);
    const at = "2026-10-19T14:00:00Z";
    const schedule: ScheduleSummary = {
      id: "running-schedule",
      workspaceId: "workspace-a",
      name: "Daily release",
      prompt: "Original instructions",
      cadence: { type: "cron", expression: "0 22 * * *", timezone: "Asia/Shanghai" },
      target: { type: "agent", agentId },
      status: "active",
      createdAt: at,
      updatedAt: at,
      nextRunAt: null,
      lastRunAt: at,
      pausedAt: null,
      maxRuns: null,
      expiresAt: null,
    };
    updateSchedule.mockRejectedValueOnce(Object.assign(new Error("Storage unavailable"), { code }));
    const onClose = vi.fn();
    render(
      <WorkspaceScheduleForm
        serverId="host-a"
        workspace={{ id: "workspace-a", name: "Feature", cwd: "/worktrees/feature" }}
        schedule={schedule}
        onClose={onClose}
      />,
    );
    fireEvent.change(screen.getByRole("textbox", { name: i18n.t("schedules.form.prompt") }), {
      target: { value: "Updated instructions" },
    });
    fireEvent.click(screen.getByRole("button", { name: i18n.t("schedules.save") }));
    await waitFor(() => expect(screen.getByText(expected)).toBeTruthy());
    expect(updateSchedule).toHaveBeenCalledWith(
      expect.objectContaining({ prompt: "Updated instructions", target: schedule.target }),
    );
    expect(onClose).not.toHaveBeenCalled();
  });

  it("preserves the exact stored time when a legacy single trigger becomes recurring", async () => {
    const at = "2035-01-01T09:00:45.123Z";
    const schedule = {
      id: "legacy-once",
      workspaceId: "workspace-a",
      name: "Legacy",
      prompt: "Review",
      cadence: { type: "once" as const, at },
      target: { type: "agent" as const, agentId },
      status: "paused" as const,
      createdAt: at,
      updatedAt: at,
      nextRunAt: null,
      lastRunAt: null,
      pausedAt: at,
      maxRuns: null,
      expiresAt: null,
    };
    render(
      <WorkspaceScheduleForm
        serverId="host-a"
        workspace={{ id: "workspace-a", name: "Feature", cwd: "/worktrees/feature" }}
        schedule={schedule}
        onClose={vi.fn()}
      />,
    );
    expect((screen.getByRole("textbox", { name: "Max runs" }) as HTMLInputElement).value).toBe("1");
    expect((screen.getByLabelText("Execution agent") as HTMLSelectElement).value).toBe(agentId);
    expect(screen.queryByRole("button", { name: "Choose model" })).toBeNull();
    fireEvent.change(screen.getByRole("textbox", { name: "Max runs" }), { target: { value: "2" } });
    fireEvent.click(screen.getByRole("button", { name: /save/i }));
    await waitFor(() =>
      expect(updateSchedule).toHaveBeenCalledWith(
        expect.objectContaining({
          cadence: { type: "every", everyMs: 3_600_000 },
          startAt: at,
          maxRuns: 2,
        }),
      ),
    );
  });
});

function seedScopeProjects() {
  for (const [serverId, projectId, projectName] of [
    ["host-a", "project-a", "Alpha"],
    ["host-a", "project-b", "Beta"],
    ["host-b", "project-a", "Remote Alpha"],
  ]) {
    projects.push({
      viewKey: `${serverId}:${projectId}`,
      projectName,
      totalWorkspaceCount: 1,
      hostCount: 1,
      onlineHostCount: 1,
      hosts: [
        {
          serverId,
          projectId,
          projectName,
          projectCustomName: null,
          serverName: serverId,
          isOnline: true,
          repoRoot: "/repo/root",
          workspaceCount: 1,
          workspaces: [],
        },
      ],
    });
  }
}

describe("global schedule placement", () => {
  beforeEach(() => {
    vi.stubGlobal("React", React);
    seedScopeProjects();
  });
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
    vi.unstubAllGlobals();
    projects.length = 0;
  });

  it("selects host, project and workspace before using the shared task form", async () => {
    render(<ScheduleFormSheet visible mode="create" onClose={vi.fn()} />);
    expect((screen.getByLabelText("Project") as HTMLSelectElement).disabled).toBe(true);
    expect((screen.getByLabelText("Workspace") as HTMLSelectElement).disabled).toBe(true);
    expect(screen.queryByLabelText("Execution agent")).toBeNull();
    fireEvent.change(screen.getByLabelText("Host"), { target: { value: "host-a" } });
    expect(screen.queryByText("Remote Alpha")).toBeNull();
    fireEvent.change(screen.getByLabelText("Project"), { target: { value: "project-a" } });
    expect(screen.queryByText("Other project workspace")).toBeNull();
    expect(screen.queryByText("Other host workspace")).toBeNull();
    fireEvent.change(screen.getByLabelText("Workspace"), { target: { value: "workspace-a" } });
    expect(screen.getAllByLabelText("Workspace")).toHaveLength(1);
    expect(screen.queryByTestId("schedule-workspace-target")).toBeNull();
    fireEvent.change(screen.getByLabelText("Execution agent"), { target: { value: agentId } });
    expect(screen.queryByRole("button", { name: "Choose model" })).toBeNull();
    fireEvent.change(screen.getByRole("textbox", { name: "Prompt" }), {
      target: { value: "Review this selected workspace" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Create schedule" }));
    await waitFor(() =>
      expect(createSchedule).toHaveBeenCalledWith(
        expect.objectContaining({
          workspaceId: "workspace-a",
          target: { type: "agent", agentId },
          maxRuns: 1,
          prompt: "Review this selected workspace",
        }),
      ),
    );
  });

  it("clears dependent choices when changing project or host, even when ids and paths coincide", () => {
    render(<ScheduleFormSheet visible mode="create" onClose={vi.fn()} />);
    fireEvent.change(screen.getByLabelText("Host"), { target: { value: "host-a" } });
    fireEvent.change(screen.getByLabelText("Project"), { target: { value: "project-a" } });
    fireEvent.change(screen.getByLabelText("Workspace"), { target: { value: "workspace-a" } });
    fireEvent.change(screen.getByLabelText("Execution agent"), { target: { value: agentId } });
    fireEvent.change(screen.getByRole("textbox", { name: "Prompt" }), {
      target: { value: "Old workspace prompt" },
    });
    fireEvent.change(screen.getByLabelText("Project"), { target: { value: "project-b" } });
    expect((screen.getByLabelText("Workspace") as HTMLSelectElement).value).toBe("");
    expect(screen.queryByLabelText("Execution agent")).toBeNull();
    fireEvent.change(screen.getByLabelText("Workspace"), { target: { value: "workspace-b" } });
    expect((screen.getByRole("textbox", { name: "Prompt" }) as HTMLInputElement).value).toBe("");
    expect(screen.queryByText("My agent")).toBeNull();
    expect(screen.getByText("Foreign agent")).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Host"), { target: { value: "host-b" } });
    expect((screen.getByLabelText("Project") as HTMLSelectElement).value).toBe("");
    expect((screen.getByLabelText("Workspace") as HTMLSelectElement).value).toBe("");
    expect((screen.getByLabelText("Workspace") as HTMLSelectElement).disabled).toBe(true);
    expect(screen.queryByLabelText("Execution agent")).toBeNull();
    fireEvent.change(screen.getByLabelText("Project"), { target: { value: "project-a" } });
    fireEvent.change(screen.getByLabelText("Workspace"), { target: { value: "workspace-a" } });
    expect(screen.queryByText("My agent")).toBeNull();
    expect(screen.getByText("Other host agent")).toBeTruthy();
  });
});

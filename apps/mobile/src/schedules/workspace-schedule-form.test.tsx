// @vitest-environment jsdom
import React, { useEffect } from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { create } from "zustand";
import { WorkspaceScheduleForm } from "@/components/schedules/schedule-form-sheet";
import type { ScheduleFormModel } from "./schedule-form-model";

const { createSchedule, updateSchedule, updatePreferences } = vi.hoisted(() => ({
  createSchedule: vi.fn(async () => ({ id: "schedule-created" })),
  updateSchedule: vi.fn(async () => {}),
  updatePreferences: vi.fn(async () => {}),
}));
const hosts = [
  { serverId: "host-a", label: "Host A" },
  { serverId: "host-b", label: "Host B" },
];
const projects: never[] = [];
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
];
const preferences = {};

vi.mock("@/hooks/use-projects", () => ({ useProjects: () => ({ projects }) }));
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
  AdaptiveModalSheet: () => <div data-testid="schedule-form-sheet" />,
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
  }: {
    label: string;
    triggerTestID?: string;
    options: { id: string; value: string; label: string }[];
    value: string;
    onChange: (value: string) => void;
  }) => (
    <select
      aria-label={label}
      data-testid={triggerTestID}
      value={value ?? ""}
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
  beforeEach(() => {
    vi.stubGlobal("React", React);
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
    vi.unstubAllGlobals();
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
    fireEvent.change(screen.getByLabelText("Target"), { target: { value: "agent" } });
    expect(screen.queryByText("Foreign agent")).toBeNull();
    expect(screen.queryByText("Archived agent")).toBeNull();
    fireEvent.change(screen.getByLabelText("Existing agent"), { target: { value: agentId } });
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
    fireEvent.change(screen.getByLabelText("Target"), { target: { value: "agent" } });
    fireEvent.change(screen.getByLabelText("Existing agent"), { target: { value: agentId } });
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

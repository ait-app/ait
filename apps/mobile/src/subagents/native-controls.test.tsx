/** @vitest-environment jsdom */
import React from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { NativeSubagentControls } from "./native-controls";
import type { PaseoSubagentRow } from "./select";

const fixture = vi.hoisted(() => ({
  guide: vi.fn(),
  stop: vi.fn(),
  enabled: true,
  health: "current",
  observationStatus: "running",
  parentManagement: "managed",
  parentStatus: "running",
  supportsChildGuide: true,
  supportsChildStop: true,
}));

vi.mock("@/stores/session-store", () => ({
  useSessionStore: (select: (state: unknown) => unknown) =>
    select({
      sessions: {
        host: {
          serverInfo: { features: { subagentControls: fixture.enabled } },
          agents: new Map([
            [
              "parent",
              {
                id: "parent",
                provider: "omp",
                status: fixture.parentStatus,
                management: fixture.parentManagement,
                capabilities: {
                  supportsChildGuide: fixture.supportsChildGuide,
                  supportsChildStop: fixture.supportsChildStop,
                },
              },
            ],
            ["worker", { id: "worker", parentAgentId: "parent" }],
          ]),
          agentDetails: new Map(),
        },
      },
    }),
}));
vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeClient: () => ({ guideSubagent: fixture.guide, stopSubagent: fixture.stop }),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: { name: string }) => (values?.name ? `${key} ${values.name}` : key),
  }),
}));
vi.mock("@/components/ui/pane-content-toolbar", () => ({
  paneContentToolbarIconSize: () => 14,
  ToolbarButton: ({
    label,
    onPress,
    testID,
  }: {
    label: string;
    onPress: (event: { stopPropagation: () => void }) => void;
    testID: string;
  }) => (
    <button data-testid={testID} onClick={() => onPress({ stopPropagation: vi.fn() })}>
      {label}
    </button>
  ),
}));
vi.mock("@/components/ui/button", () => ({
  Button: ({
    children,
    onPress,
    disabled,
    testID,
  }: {
    children: React.ReactNode;
    onPress: () => void;
    disabled: boolean;
    testID: string;
  }) => (
    <button data-testid={testID} onClick={onPress} disabled={disabled}>
      {children}
    </button>
  ),
}));
vi.mock("@/components/adaptive-modal-sheet", () => ({
  AdaptiveModalSheet: ({
    children,
    footer,
    header,
  }: {
    children: React.ReactNode;
    footer: React.ReactNode;
    header: { title: string };
  }) => (
    <div role="dialog">
      <span>{header.title}</span>
      {children}
      {footer}
    </div>
  ),
  AdaptiveTextInput: ({
    initialValue,
    onChangeText,
    testID,
  }: {
    initialValue: string;
    onChangeText: (text: string) => void;
    testID: string;
  }) => (
    <textarea
      data-testid={testID}
      defaultValue={initialValue}
      onChange={(event) => onChangeText(event.target.value)}
    />
  ),
}));

function row(overrides: Partial<PaseoSubagentRow> = {}): PaseoSubagentRow {
  return {
    kind: "paseo",
    id: "worker",
    provider: "omp",
    title: "Build handoff card",
    description: "Implementation",
    subtitle: null,
    status: "running",
    turn: { phase: "open", turnId: "turn", startedAt: new Date(1), cancellationRequestId: null },
    requiresAttention: false,
    createdAt: new Date(1),
    observation: {
      rootAgentId: "parent",
      nativeId: "native-worker",
      status: fixture.observationStatus as "running",
      health: fixture.health as "current",
      observedAt: new Date(1).toISOString(),
      description: null,
      subtitle: null,
    },
    ...overrides,
  };
}

/** Restore the permissive default so each case isolates the one fact it removes. */
function reset() {
  fixture.enabled = true;
  fixture.health = "current";
  fixture.observationStatus = "running";
  fixture.parentManagement = "managed";
  fixture.parentStatus = "running";
  fixture.supportsChildGuide = true;
  fixture.supportsChildStop = true;
}

beforeEach(() => {
  fixture.guide.mockReset().mockResolvedValue({ disposition: "accepted" });
  fixture.stop.mockReset().mockResolvedValue({ disposition: "cancelled" });
  reset();
});
afterEach(cleanup);

it("guides the observed child through its parent and reports the typed receipt", async () => {
  render(<NativeSubagentControls serverId="host" row={row()} compact={false} />);
  fireEvent.click(screen.getByTestId("native-guide-worker"));
  expect(screen.getByTestId("native-control-confirm")).toHaveProperty("disabled", true);
  fireEvent.change(screen.getByTestId("native-control-guidance"), {
    target: { value: "Check keyboard navigation" },
  });
  await act(async () => fireEvent.click(screen.getByTestId("native-control-confirm")));
  expect(fixture.guide).toHaveBeenCalledExactlyOnceWith(
    "parent",
    "worker",
    "Check keyboard navigation",
  );
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(screen.getByTestId("native-notice-worker").textContent).toBe("providerControl.tasks.sent");
  expect(fixture.stop).not.toHaveBeenCalled();
});

it("a stop receipt is not a stopped child while the observation is still running", async () => {
  render(<NativeSubagentControls serverId="host" row={row()} compact={false} />);
  fireEvent.click(screen.getByTestId("native-stop-worker"));
  await act(async () => fireEvent.click(screen.getByTestId("native-control-confirm")));
  expect(fixture.stop).toHaveBeenCalledExactlyOnceWith("parent", "worker");
  expect(screen.getByTestId("native-notice-worker").textContent).toBe(
    "providerControl.tasks.stopped",
  );
});

it("a not-running answer is its own fact, never a delivery", async () => {
  fixture.guide.mockResolvedValue({ disposition: "not_running" });
  render(<NativeSubagentControls serverId="host" row={row()} compact={false} />);
  fireEvent.click(screen.getByTestId("native-guide-worker"));
  fireEvent.change(screen.getByTestId("native-control-guidance"), {
    target: { value: "Keep going" },
  });
  await act(async () => fireEvent.click(screen.getByTestId("native-control-confirm")));
  expect(screen.getByTestId("native-notice-worker").textContent).toBe(
    "providerControl.tasks.notRunningGuide",
  );
});

it("reports a refusal without leaking provider detail or closing the sheet", async () => {
  fixture.stop.mockRejectedValue(new Error("native stderr: cancelled=undefined"));
  render(<NativeSubagentControls serverId="host" row={row()} compact={false} />);
  fireEvent.click(screen.getByTestId("native-stop-worker"));
  await act(async () => fireEvent.click(screen.getByTestId("native-control-confirm")));
  const error = screen.getByTestId("native-control-error");
  expect(error.textContent).toBe("providerControl.tasks.stopFailed");
  expect(error.textContent).not.toContain("native");
  expect(screen.getByRole("dialog")).not.toBeNull();
  expect(screen.queryByTestId("native-notice-worker")).toBeNull();
});

it("exposes no control without a feature, a managed parent writer or a current running child", () => {
  // Each case removes the one fact its name states; every other fact stays permissive.
  const cases: [name: string, mutate: () => void, expected: readonly string[]][] = [
    ["host feature absent", () => (fixture.enabled = false), []],
    ["observed parent", () => (fixture.parentManagement = "observed"), []],
    ["closed parent", () => (fixture.parentStatus = "closed"), []],
    ["stale observation", () => (fixture.health = "stale"), []],
    ["terminal observation", () => (fixture.observationStatus = "completed"), []],
    ["only guide supported", () => (fixture.supportsChildStop = false), ["native-guide-worker"]],
    ["only stop supported", () => (fixture.supportsChildGuide = false), ["native-stop-worker"]],
  ];
  for (const [name, mutate, expected] of cases) {
    reset();
    mutate();
    const result = render(<NativeSubagentControls serverId="host" row={row()} compact={false} />);
    expect(
      screen.queryAllByTestId(/^native-(?:guide|stop)-worker$/).map((node) => node.dataset.testid),
      name,
    ).toEqual(expected);
    result.unmount();
  }
  expect(fixture.guide).not.toHaveBeenCalled();
  expect(fixture.stop).not.toHaveBeenCalled();
});

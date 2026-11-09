// @vitest-environment jsdom
import React from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { NativeSubagentControls } from "./native-controls";
import type { PaseoSubagentRow } from "./select";
const fixture = vi.hoisted(() => ({
  run: vi.fn(),
  enabled: true,
  health: "current",
  workerStatus: "running",
}));
vi.mock("@/stores/session-store", () => ({
  useSessionStore: (select: (state: unknown) => unknown) =>
    select({
      sessions: {
        host: {
          agents: new Map([
            [
              "parent",
              {
                providerUi: {
                  actions: [{ id: "vibe_control" }],
                  vibe: {
                    enabled: fixture.enabled,
                    workers: [{ id: "worker", status: fixture.workerStatus }],
                  },
                },
              },
            ],
          ]),
        },
      },
    }),
}));
vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeClient: () => ({ runNativeAction: fixture.run }),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: { name: string }) => (values?.name ? `${key} ${values.name}` : key),
  }),
}));
vi.mock("react-native-unistyles", () => ({
  StyleSheet: { create: (fn: (theme: unknown) => unknown) => fn({ colors: {} }) },
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
const row: PaseoSubagentRow = {
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
    status: "running",
    health: "current",
    observedAt: new Date(1).toISOString(),
    description: null,
    subtitle: null,
  },
};
beforeEach(() => {
  fixture.enabled = true;
  fixture.workerStatus = "running";
  fixture.run.mockReset().mockResolvedValue({ text: "Accepted" });
});
afterEach(cleanup);
it("guidance is bound to the selected task and uses only the parent writer", async () => {
  render(<NativeSubagentControls serverId="host" row={row} compact={false} />);
  fireEvent.click(screen.getByTestId("native-guide-worker"));
  expect(screen.getByTestId("native-worker-confirm")).toHaveProperty("disabled", true);
  fireEvent.change(screen.getByTestId("native-worker-guidance"), {
    target: { value: "Check keyboard navigation" },
  });
  await act(async () => fireEvent.click(screen.getByTestId("native-worker-confirm")));
  expect(fixture.run).toHaveBeenCalledExactlyOnceWith("parent", "vibe_control", {
    operation: "send",
    sessionId: "worker",
    message: "Check keyboard navigation",
  });
  expect(screen.queryByRole("dialog")).toBeNull();
});
it("stopping asks confirmation with task name and cannot send duplicate requests", async () => {
  let finish!: (value: unknown) => void;
  fixture.run.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  render(<NativeSubagentControls serverId="host" row={row} compact={false} />);
  fireEvent.click(screen.getByTestId("native-stop-worker"));
  expect(fixture.run).not.toHaveBeenCalled();
  expect(screen.getByRole("dialog").textContent).toContain(row.title);
  fireEvent.click(screen.getByTestId("native-worker-confirm"));
  fireEvent.click(screen.getByTestId("native-worker-confirm"));
  expect(fixture.run).toHaveBeenCalledExactlyOnceWith("parent", "vibe_control", {
    operation: "kill",
    sessionId: "worker",
  });
  await act(async () => finish({ text: "Stopped" }));
});
it("stale or disabled Vibe observations expose no mutation controls", () => {
  const stale = { ...row, observation: { ...row.observation!, health: "stale" as const } };
  const result = render(<NativeSubagentControls serverId="host" row={stale} compact={false} />);
  expect(screen.queryByTestId("native-guide-worker")).toBeNull();
  result.unmount();
  fixture.enabled = false;
  render(<NativeSubagentControls serverId="host" row={row} compact={false} />);
  expect(screen.queryByTestId("native-stop-worker")).toBeNull();
  expect(fixture.run).not.toHaveBeenCalled();
});

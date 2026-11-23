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
  connected: true,
  guest: false,
  guestScope: "workspace",
  accountId: "visitor-1",
  accountCenter: "https://center.test",
  accountStatus: "online",
  parentShare: "share-1",
  parentUser: "visitor-1",
  activeConnectionId: "connection",
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
                labels: {
                  "ait.guest.share-id": fixture.parentShare,
                  "ait.guest.user-id": fixture.parentUser,
                },
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
  useHostRuntimeIsConnected: () => fixture.connected,
  useHostRuntimeSnapshot: () => ({ activeConnectionId: fixture.activeConnectionId }),
  useHosts: () => [
    {
      serverId: "host",
      connections: fixture.guest
        ? [
            {
              id: "connection",
              type: "accountRelay",
              shareId: "share-1",
              shareResourceKind: fixture.guestScope,
              center: "https://center.test",
            },
          ]
        : [{ id: "connection", type: "directTcp" }],
    },
  ],
}));
vi.mock("@/runtime/account-state", () => ({
  useAccountState: () => ({
    status: fixture.accountStatus,
    center: fixture.accountCenter,
    accountId: fixture.accountId,
  }),
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
      revision: 7,
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
  fixture.connected = true;
  fixture.guest = false;
  fixture.guestScope = "workspace";
  fixture.accountId = "visitor-1";
  fixture.accountCenter = "https://center.test";
  fixture.accountStatus = "online";
  fixture.parentShare = "share-1";
  fixture.parentUser = "visitor-1";
  fixture.activeConnectionId = "connection";
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
    { expectedRevision: 7 },
  );
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(screen.getByTestId("native-notice-worker").textContent).toBe("providerControl.tasks.sent");
  expect(fixture.stop).not.toHaveBeenCalled();
});

it("a stop receipt is not a stopped child while the observation is still running", async () => {
  render(<NativeSubagentControls serverId="host" row={row()} compact={false} />);
  fireEvent.click(screen.getByTestId("native-stop-worker"));
  await act(async () => fireEvent.click(screen.getByTestId("native-control-confirm")));
  expect(fixture.stop).toHaveBeenCalledExactlyOnceWith("parent", "worker", { expectedRevision: 7 });
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
    ["host offline", () => (fixture.connected = false), []],
    [
      "other guest's agent",
      () => {
        fixture.guest = true;
        fixture.parentUser = "other-user";
      },
      [],
    ],
    [
      "other share's agent",
      () => {
        fixture.guest = true;
        fixture.parentShare = "other-share";
      },
      [],
    ],
    [
      "guest without account identity",
      () => {
        fixture.guest = true;
        fixture.accountId = "";
      },
      [],
    ],
    [
      "guest on another center",
      () => {
        fixture.guest = true;
        fixture.accountCenter = "https://other.test";
      },
      [],
    ],
    [
      "guest identity offline",
      () => {
        fixture.guest = true;
        fixture.accountStatus = "logged_out";
      },
      [],
    ],
    [
      "unknown guest scope",
      () => {
        fixture.guest = true;
        fixture.guestScope = "unknown";
      },
      [],
    ],
    ["missing active connection", () => (fixture.activeConnectionId = ""), []],
    [
      "workspace guest's own agent",
      () => {
        fixture.guest = true;
      },
      ["native-guide-worker", "native-stop-worker"],
    ],
    [
      "project guest's own agent",
      () => {
        fixture.guest = true;
        fixture.guestScope = "project";
      },
      ["native-guide-worker", "native-stop-worker"],
    ],
    [
      "whole-node guest",
      () => {
        fixture.guest = true;
        fixture.guestScope = "node";
        fixture.parentUser = "other-user";
      },
      ["native-guide-worker", "native-stop-worker"],
    ],
    [
      "idle parent with live writer",
      () => (fixture.parentStatus = "idle"),
      ["native-guide-worker", "native-stop-worker"],
    ],
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

it("preserves guidance whitespace and refuses UTF-8 payloads beyond the limit", async () => {
  render(<NativeSubagentControls serverId="host" row={row()} compact={false} />);
  fireEvent.click(screen.getByTestId("native-guide-worker"));
  fireEvent.change(screen.getByTestId("native-control-guidance"), {
    target: { value: "界".repeat(21846) },
  });
  expect(screen.getByTestId("native-control-confirm")).toHaveProperty("disabled", true);
  expect(screen.getByText("providerControl.tasks.tooLong")).toBeTruthy();
  fireEvent.change(screen.getByTestId("native-control-guidance"), {
    target: { value: "  keep whitespace\n" },
  });
  await act(async () => fireEvent.click(screen.getByTestId("native-control-confirm")));
  expect(fixture.guide).toHaveBeenCalledExactlyOnceWith("parent", "worker", "  keep whitespace\n", {
    expectedRevision: 7,
  });
});

it("does not submit the same guidance twice while a receipt is pending", async () => {
  const response = Promise.withResolvers<{ disposition: string }>();
  fixture.guide.mockReturnValueOnce(response.promise);
  render(<NativeSubagentControls serverId="host" row={row()} compact={false} />);
  fireEvent.click(screen.getByTestId("native-guide-worker"));
  fireEvent.change(screen.getByTestId("native-control-guidance"), {
    target: { value: "Review output" },
  });
  fireEvent.click(screen.getByTestId("native-control-confirm"));
  fireEvent.click(screen.getByTestId("native-control-confirm"));
  expect(fixture.guide).toHaveBeenCalledOnce();
  await act(async () => response.resolve({ disposition: "accepted" }));
});

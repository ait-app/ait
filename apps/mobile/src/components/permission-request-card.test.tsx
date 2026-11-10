// @vitest-environment jsdom
import React from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import type { PendingPermission } from "@/types/shared";
import { PermissionRequestCard } from "./permission-request-card";

vi.mock("@/constants/layout", () => ({ useIsCompactFormFactor: () => false }));
vi.mock("@/components/tool-call-details", () => ({
  ToolCallDetailsContent: ({ detail }: { detail: unknown }) => (
    <pre data-testid="tool-detail">{JSON.stringify(detail)}</pre>
  ),
}));
vi.mock("@/components/plan-card", () => ({
  PlanCard: ({ footer }: { footer: React.ReactNode }) => <div>{footer}</div>,
}));
vi.mock("@/components/question-form-card", () => ({
  QuestionFormCard: ({
    onRespond,
    isResponding,
  }: {
    onRespond: (response: unknown) => void;
    isResponding: boolean;
  }) => (
    <button
      disabled={isResponding}
      onClick={() => onRespond({ behavior: "allow", updatedInput: { answers: { choice: "yes" } } })}
    >
      Answer question
    </button>
  ),
}));
vi.mock("@/components/ui/button", () => ({
  Button: ({
    onPress,
    children,
    disabled,
    testID,
  }: {
    onPress: () => void;
    children: React.ReactNode;
    disabled: boolean;
    testID: string;
  }) => (
    <button data-testid={testID} onClick={onPress} disabled={disabled}>
      {children}
    </button>
  ),
}));

const permission: PendingPermission = {
  key: "request-key",
  agentId: "agent-1",
  request: {
    id: "request-1",
    provider: "codex",
    name: "shell",
    kind: "tool",
    title: "Run tests?",
    actions: [
      { id: "provider-allow-once", label: "Allow once", behavior: "allow", variant: "primary" },
      { id: "provider-deny", label: "Deny", behavior: "deny", variant: "danger" },
    ],
  },
};

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

function mount(client: unknown, request = permission) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <PermissionRequestCard panel permission={request} client={client as DaemonClient | null} />
    </QueryClientProvider>,
  );
}

describe("shared permission card", () => {
  it("sends the provider action ID and locks duplicate responses until resolution", async () => {
    let finish!: (value: unknown) => void;
    const respondToPermissionAndWait = vi.fn(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    mount({ respondToPermissionAndWait });
    fireEvent.click(screen.getByText("Allow once"));
    await waitFor(() =>
      expect(respondToPermissionAndWait).toHaveBeenCalledWith(
        "agent-1",
        "request-1",
        { behavior: "allow", selectedActionId: "provider-allow-once" },
        15000,
      ),
    );
    expect((screen.getByText("Deny") as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByText("Allow once"));
    expect(respondToPermissionAndWait).toHaveBeenCalledTimes(1);
    finish({});
    await waitFor(() =>
      expect((screen.getByText("Allow once") as HTMLButtonElement).disabled).toBe(true),
    );
  });

  it("shows errors and permits retrying the exact denial action", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const respondToPermissionAndWait = vi
      .fn()
      .mockRejectedValueOnce(new Error("Connection closed"))
      .mockResolvedValue({});
    mount({ respondToPermissionAndWait });
    fireEvent.click(screen.getByText("Deny"));
    await screen.findByRole("alert");
    expect(screen.getByRole("alert").textContent).toBe("Connection closed");
    expect((screen.getByText("Deny") as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(screen.getByText("Deny"));
    await waitFor(() => expect(respondToPermissionAndWait).toHaveBeenCalledTimes(2));
    expect(respondToPermissionAndWait.mock.calls[1][2]).toEqual({
      behavior: "deny",
      selectedActionId: "provider-deny",
      message: "Denied by user",
    });
  });

  it("disables approval and question submission without a connected client", () => {
    const card = mount(null);
    expect((screen.getByText("Allow once") as HTMLButtonElement).disabled).toBe(true);
    card.unmount();
    mount(null, { ...permission, request: { ...permission.request, kind: "question" } });
    expect((screen.getByText("Answer question") as HTMLButtonElement).disabled).toBe(true);
  });

  it("shows the exact command and working directory without transport IDs", () => {
    mount(null, {
      ...permission,
      request: {
        ...permission.request,
        name: "commandExecution",
        input: { command: "npm test -- login", cwd: "/repo", itemId: "internal-transport-id" },
      },
    });
    expect(JSON.parse(screen.getByTestId("tool-detail").textContent!)).toEqual({
      type: "shell",
      command: "npm test -- login",
      cwd: "/repo",
    });
  });

  it("retains unrecognized command arguments for permission review", () => {
    const input = { command: "npm test", cwd: "/repo", env: { TARGET: "production" } };
    mount(null, {
      ...permission,
      request: { ...permission.request, name: "commandExecution", input },
    });
    expect(JSON.parse(screen.getByTestId("tool-detail").textContent!).input).toEqual(input);
  });

  it("retains question answers and uses the bounded question response deadline", async () => {
    const respondToPermissionAndWait = vi.fn().mockResolvedValue({});
    mount(
      { respondToPermissionAndWait },
      { ...permission, request: { ...permission.request, kind: "question" } },
    );
    fireEvent.click(screen.getByText("Answer question"));
    await waitFor(() =>
      expect(respondToPermissionAndWait).toHaveBeenCalledWith(
        "agent-1",
        "request-1",
        { behavior: "allow", updatedInput: { answers: { choice: "yes" } } },
        5000,
      ),
    );
  });
});

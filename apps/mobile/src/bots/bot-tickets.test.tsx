// @vitest-environment jsdom
import React from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BotTicket, BotTicketSummary, BotView } from "@ait/protocol/bot/types";
import { BotTicketSheet } from "./bot-ticket-sheet";
import { BotTicketsTrack } from "./bot-tickets";

const { botRequest, confirmDialog } = vi.hoisted(() => ({
  botRequest: vi.fn(),
  confirmDialog: vi.fn(async (_options: unknown) => true),
}));
vi.mock("@/runtime/host-runtime", () => ({ useHostRuntimeClient: () => ({ botRequest }) }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options ? `${key} ${JSON.stringify(options)}` : key,
  }),
}));
vi.mock("./styles", () => ({ botStyles: {}, botTicketStyles: {} }));
vi.mock("./loading", () => ({ BotLoadingSpinner: () => null }));
vi.mock("@/utils/confirm-dialog", () => ({ confirmDialog }));
vi.mock("@/stores/session-store", () => ({
  useSessionStore: (select: (state: unknown) => unknown) =>
    select({
      sessions: {
        host: {
          serverInfo: { features: { workspaceBotTickets: true } },
          agents: new Map([["agt_1", { title: "Login fixer" }]]),
        },
      },
    }),
}));
vi.mock("@/composer/tracks", () => ({
  ComposerTrackPill: ({
    segments,
    accessibilityLabel,
    testID,
    children,
  }: {
    segments: { bucket: string | null; text: string }[];
    accessibilityLabel: string;
    testID: string;
    children: React.ReactNode;
  }) => (
    <div>
      <button data-testid={testID} aria-label={accessibilityLabel}>
        {segments.map((segment) => (
          <span key={segment.text} data-bucket={segment.bucket ?? "none"}>
            {segment.text}
          </span>
        ))}
      </button>
      <div data-testid={`${testID}-panel`}>{children}</div>
    </div>
  ),
  ComposerTrackRow: ({
    children,
    onPress,
    testID,
  }: {
    children: React.ReactNode;
    onPress?: () => void;
    testID?: string;
  }) =>
    onPress ? (
      <button data-testid={testID} onClick={onPress}>
        {children}
      </button>
    ) : (
      <div>{children}</div>
    ),
}));
vi.mock("@/components/adaptive-modal-sheet", () => ({
  AdaptiveModalSheet: ({
    header,
    children,
    footer,
    onClose,
  }: {
    header: { title: string };
    children: React.ReactNode;
    footer?: React.ReactNode;
    onClose: () => void;
  }) => (
    <div data-testid="sheet">
      <h1>{header.title}</h1>
      <button data-testid="sheet-close" onClick={onClose} />
      {children}
      <div data-testid="sheet-footer">{footer}</div>
    </div>
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
    disabled?: boolean;
    testID?: string;
  }) => (
    <button data-testid={testID} disabled={disabled} onClick={onPress}>
      {children}
    </button>
  ),
}));
vi.mock("@/components/ui/form-field", () => ({
  FormTextInput: ({
    onChangeText,
    editable,
    testID,
  }: {
    onChangeText: (text: string) => void;
    editable: boolean;
    testID: string;
  }) => (
    <textarea
      data-testid={testID}
      disabled={!editable}
      onChange={(event) => onChangeText(event.target.value)}
    />
  ),
}));

const at = "2026-11-13T08:00:00Z";
const bot: BotView = {
  workspaceId: "workspace",
  execution: { provider: "codex", model: null, modeId: null, thinking: null },
  builtinChannelId: "22222222-2222-4222-8222-222222222222",
  state: { kind: "active" },
  createdAt: at,
};
const ticketId = "11111111-1111-4111-8111-111111111111";

function summary(id: string, patch: Partial<BotTicketSummary> = {}): BotTicketSummary {
  return {
    id,
    title: `Ticket ${id}`,
    status: "open",
    waiting: null,
    conversationId: null,
    createdAt: at,
    updatedAt: at,
    closedAt: null,
    outcome: null,
    assignments: [],
    latestReport: null,
    ...patch,
  };
}

function ticket(patch: Partial<BotTicket> = {}): BotTicket {
  return {
    id: ticketId,
    title: "Fix login",
    status: "waiting",
    waiting: { reason: "blocked", agentId: "agt_1", since: at },
    conversationId: null,
    createdAt: at,
    updatedAt: at,
    closedAt: null,
    outcome: null,
    assignments: [
      { agentId: "agt_1", workspaceId: "workspace", assignedAt: at },
      { agentId: "agt_elsewhere", workspaceId: "other", assignedAt: at },
    ],
    request: "Fix the login form",
    reports: [
      {
        sequence: 1,
        at,
        source: "agent",
        kind: "progress",
        agentId: "agt_1",
        text: "Found the bug",
        status: null,
      },
      {
        sequence: 2,
        at,
        source: "agent",
        kind: "blocked",
        agentId: "agt_1",
        text: "Which provider should log in?",
        status: "waiting",
      },
    ],
    ...patch,
  };
}

let queryClient: QueryClient;
beforeEach(() => {
  queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  botRequest.mockReset();
  confirmDialog.mockClear();
  confirmDialog.mockResolvedValue(true);
});
afterEach(() => {
  cleanup();
  queryClient.clear();
});

function wrap(children: React.ReactNode) {
  return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
}

describe("Bot tasks pill", () => {
  it("counts open tasks, puts waiting ones first and says why they wait", async () => {
    const open = summary("open", {
      assignments: [{ agentId: "agt_2", workspaceId: "workspace", assignedAt: at }],
      latestReport: {
        sequence: 3,
        at,
        source: "agent",
        kind: "progress",
        agentId: "agt_2",
        text: "Halfway",
        status: null,
      },
    });
    const waiting = summary("waiting", {
      status: "waiting",
      waiting: { reason: "permission", agentId: "agt_1", since: at },
      latestReport: {
        sequence: 1,
        at,
        source: "system",
        kind: "agent_permission",
        agentId: "agt_1",
        text: "",
        status: "waiting",
      },
    });
    botRequest.mockResolvedValue({ tickets: [open, waiting] });
    const onOpenTicket = vi.fn();
    render(
      wrap(
        <BotTicketsTrack
          serverId="host"
          workspaceId="workspace"
          bot={bot}
          interactive
          onOpenTicket={onOpenTicket}
        />,
      ),
    );
    await screen.findByTestId("bot-ticket-row-waiting");
    expect(botRequest).toHaveBeenCalledWith("bot.ticket.list.request", {
      workspaceId: "workspace",
      limit: 50,
    });
    const pill = screen.getByTestId("bot-tickets");
    expect(pill.textContent).toBe(
      'bots.tickets.pillWaiting {"count":1}bots.tickets.pillOpen {"count":2}',
    );
    expect(pill.querySelector('[data-bucket="needs_input"]')).not.toBeNull();
    expect(pill.getAttribute("aria-label")).toBe('bots.tickets.pillLabel {"open":2,"waiting":1}');
    const rows = screen
      .getAllByTestId(/^bot-ticket-row-/)
      .map((row) => row.getAttribute("data-testid"));
    expect(rows).toEqual(["bot-ticket-row-waiting", "bot-ticket-row-open"]);
    expect(screen.getByTestId("bot-ticket-facts-waiting").textContent).toBe(
      'bots.tickets.waiting.permission · bots.tickets.agentCount {"count":0}',
    );
    expect(screen.getByTestId("bot-ticket-facts-open").textContent).toBe(
      'bots.tickets.status.open · bots.tickets.agentCount {"count":1}',
    );
    // A report without text names what happened instead.
    expect(screen.getByTestId("bot-ticket-latest-waiting").textContent).toBe(
      "bots.tickets.source.system: bots.tickets.report.agent_permission",
    );
    expect(screen.getByTestId("bot-ticket-latest-open").textContent).toBe(
      "bots.tickets.source.agent: Halfway",
    );
    fireEvent.click(screen.getByTestId("bot-ticket-row-waiting"));
    expect(onOpenTicket).toHaveBeenCalledWith("waiting");
  });

  it("loads recently closed tasks only on request", async () => {
    botRequest.mockImplementation(async (_method: string, params: { includeClosed?: boolean }) => ({
      tickets: params.includeClosed
        ? [summary("closed", { status: "done", closedAt: at, outcome: "Shipped" })]
        : [],
    }));
    render(
      wrap(
        <BotTicketsTrack
          serverId="host"
          workspaceId="workspace"
          bot={bot}
          interactive
          onOpenTicket={vi.fn()}
        />,
      ),
    );
    await screen.findByTestId("bot-tickets-empty");
    expect(screen.getByTestId("bot-tickets").textContent).toBe("bots.tickets.title");
    expect(screen.queryByTestId("bot-ticket-row-closed")).toBeNull();
    expect(botRequest).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByTestId("bot-tickets-toggle-closed"));
    await screen.findByTestId("bot-ticket-row-closed");
    expect(botRequest).toHaveBeenLastCalledWith("bot.ticket.list.request", {
      workspaceId: "workspace",
      includeClosed: true,
      limit: 64,
    });
    // Closed tasks are not counted as open.
    expect(screen.getByTestId("bot-tickets").textContent).toBe("bots.tickets.title");
  });

  it("makes no request while the tab is hidden", () => {
    render(
      wrap(
        <BotTicketsTrack
          serverId="host"
          workspaceId="workspace"
          bot={bot}
          interactive={false}
          onOpenTicket={vi.fn()}
        />,
      ),
    );
    expect(botRequest).not.toHaveBeenCalled();
  });
});

function renderSheet(readOnly = false, onOpenAgent = vi.fn()) {
  return render(
    wrap(
      <BotTicketSheet
        serverId="host"
        workspaceId="workspace"
        ticketId={ticketId}
        visible
        readOnly={readOnly}
        onClose={vi.fn()}
        onDismiss={vi.fn()}
        onOpenAgent={onOpenAgent}
      />,
    ),
  );
}

describe("Bot task detail", () => {
  it("shows the request, the cause, the Agents and the history in order with who said it", async () => {
    botRequest.mockResolvedValue({ ticket: ticket() });
    const onOpenAgent = vi.fn();
    renderSheet(false, onOpenAgent);
    await screen.findByTestId(`bot-ticket-${ticketId}`);
    expect(botRequest).toHaveBeenCalledWith("bot.ticket.get.request", {
      workspaceId: "workspace",
      ticketId,
    });
    expect(screen.getByText("Fix login")).toBeTruthy();
    expect(screen.getByText("Fix the login form")).toBeTruthy();
    expect(screen.getByTestId("bot-ticket-status").textContent).toBe(
      "bots.tickets.status.waiting · bots.tickets.waiting.blocked",
    );
    expect(screen.queryByTestId("bot-ticket-outcome")).toBeNull();
    // The Agent in this Workspace opens beside the chat; one elsewhere stays plain text.
    fireEvent.click(screen.getByTestId("bot-ticket-agent-agt_1"));
    expect(onOpenAgent).toHaveBeenCalledWith("agt_1");
    expect(screen.getByText("Login fixer", { selector: "a, div, span" })).toBeTruthy();
    expect(screen.queryByTestId("bot-ticket-agent-agt_elsewhere")).toBeNull();
    const reports = screen.getAllByTestId(/^bot-ticket-report-/);
    expect(reports.map((report) => report.getAttribute("data-testid"))).toEqual([
      "bot-ticket-report-1",
      "bot-ticket-report-2",
    ]);
    expect(reports[1].textContent).toContain("bots.tickets.source.agent · Login fixer");
    expect(reports[1].textContent).toContain("bots.tickets.report.blocked");
    expect(reports[1].textContent).toContain(
      'bots.tickets.movedTo {"status":"bots.tickets.status.waiting"}',
    );
    expect(reports[1].textContent).toContain("Which provider should log in?");
  });

  it("marks a task done with the optional outcome", async () => {
    const done = ticket({ status: "done", waiting: null, closedAt: at, outcome: "Shipped" });
    botRequest.mockImplementation(async (method: string) =>
      method === "bot.ticket.get.request" ? { ticket: ticket() } : { ticket: done, changed: true },
    );
    renderSheet();
    fireEvent.change(await screen.findByTestId("bot-ticket-outcome-input"), {
      target: { value: "  Shipped  " },
    });
    fireEvent.click(screen.getByTestId("bot-ticket-done"));
    await screen.findByTestId("bot-ticket-outcome");
    expect(botRequest).toHaveBeenCalledWith("bot.ticket.update.request", {
      workspaceId: "workspace",
      ticketId,
      status: "done",
      outcome: "Shipped",
    });
    expect(confirmDialog).not.toHaveBeenCalled();
    expect(screen.getByTestId("bot-ticket-outcome").textContent).toBe("Shipped");
    expect(screen.getByTestId("bot-ticket-reopen")).toBeTruthy();
    expect(screen.queryByTestId("bot-ticket-done")).toBeNull();
  });

  it("asks before cancelling and sends nothing when the owner keeps the task", async () => {
    botRequest.mockResolvedValue({ ticket: ticket() });
    confirmDialog.mockResolvedValueOnce(false);
    renderSheet();
    fireEvent.click(await screen.findByTestId("bot-ticket-cancel"));
    await waitFor(() => expect(confirmDialog).toHaveBeenCalledTimes(1));
    expect(botRequest.mock.calls.map(([method]) => method)).not.toContain(
      "bot.ticket.update.request",
    );
    const cancelled = ticket({ status: "cancelled", waiting: null, closedAt: at });
    botRequest.mockResolvedValue({ ticket: cancelled, changed: true });
    fireEvent.click(screen.getByTestId("bot-ticket-cancel"));
    await screen.findByTestId("bot-ticket-reopen");
    // An empty outcome is left out rather than sent blank.
    expect(botRequest).toHaveBeenCalledWith("bot.ticket.update.request", {
      workspaceId: "workspace",
      ticketId,
      status: "cancelled",
    });
    expect(screen.getByTestId("bot-ticket-outcome").textContent).toBe("bots.tickets.noOutcome");
  });

  it("reopens a closed task and explains the open-task limit", async () => {
    const closed = ticket({ status: "done", waiting: null, closedAt: at, outcome: "Shipped" });
    botRequest.mockImplementation(async (method: string) => {
      if (method === "bot.ticket.get.request") return { ticket: closed };
      throw Object.assign(new Error("Resource budget exhausted"), { code: "resource_exhausted" });
    });
    renderSheet();
    fireEvent.click(await screen.findByTestId("bot-ticket-reopen"));
    await screen.findByText("bots.tickets.openLimit");
    expect(botRequest).toHaveBeenCalledWith("bot.ticket.update.request", {
      workspaceId: "workspace",
      ticketId,
      status: "open",
    });
    const reopened = ticket({ status: "open", waiting: null });
    botRequest.mockImplementation(async (method: string) =>
      method === "bot.ticket.get.request"
        ? { ticket: closed }
        : { ticket: reopened, changed: true },
    );
    fireEvent.click(screen.getByTestId("bot-ticket-reopen"));
    await screen.findByTestId("bot-ticket-done");
    expect(screen.queryByTestId("bot-ticket-outcome")).toBeNull();
  });

  it("shows guests the task without any action", async () => {
    botRequest.mockResolvedValue({ ticket: ticket() });
    renderSheet(true);
    await screen.findByTestId(`bot-ticket-${ticketId}`);
    expect(screen.queryByTestId("bot-ticket-done")).toBeNull();
    expect(screen.queryByTestId("bot-ticket-cancel")).toBeNull();
    expect(screen.queryByTestId("bot-ticket-outcome-input")).toBeNull();
    expect(screen.getByTestId("sheet-footer").textContent).toBe("");
  });
});

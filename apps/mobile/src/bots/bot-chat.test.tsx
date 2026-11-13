// @vitest-environment jsdom
import React from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BotTurn, BotView } from "@ait/protocol/bot/types";
import { BotChat } from "./bot-chat";
import { botKeys } from "./queries";

const { botRequest, closeBotSessionTab, openPreferredWorkspaceTarget, tickets } = vi.hoisted(
  () => ({
    tickets: { current: false },
    botRequest: vi.fn(),
    closeBotSessionTab: vi.fn(),
    openPreferredWorkspaceTarget: vi.fn(),
  }),
);
vi.mock("@/runtime/host-runtime", () => ({ useHostRuntimeClient: () => ({ botRequest }) }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("./styles", () => ({ botStyles: {}, botChatStyles: {}, botTicketStyles: {} }));
vi.mock("./bot-tickets", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./bot-tickets")>()),
  useBotTicketsSupported: () => tickets.current,
}));
vi.mock("./bot-ticket-sheet", () => ({
  BotTicketSheet: ({
    ticketId,
    visible,
    readOnly,
    onClose,
    onDismiss,
  }: {
    ticketId: string;
    visible: boolean;
    readOnly: boolean;
    onClose: () => void;
    onDismiss: () => void;
  }) => (
    <div
      data-testid="bot-ticket-sheet"
      data-ticket={ticketId}
      data-visible={visible}
      data-read-only={readOnly}
    >
      <button data-testid="bot-ticket-sheet-close" onClick={onClose} />
      <button data-testid="bot-ticket-sheet-dismiss" onClick={onDismiss} />
    </div>
  ),
}));
vi.mock("./loading", () => ({ BotLoadingSpinner: () => null }));
vi.mock("@/components/ui/select-field", () => ({
  SelectField: ({
    value,
    options,
    onChange,
    testID,
  }: {
    value: string;
    options: { id: string; label: string }[];
    onChange: (value: string) => void;
    testID: string;
  }) => (
    <select data-testid={testID} value={value} onChange={(event) => onChange(event.target.value)}>
      {options.map((option) => (
        <option key={option.id} value={option.id}>
          {option.label}
        </option>
      ))}
    </select>
  ),
}));
vi.mock("./session-tab", () => ({ closeBotSessionTab }));
vi.mock("@/utils/confirm-dialog", () => ({ confirmDialog: vi.fn(async () => true) }));
vi.mock("@/components/markdown/renderer", () => ({
  MarkdownRenderer: ({ text }: { text: string }) => <span>{text}</span>,
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
vi.mock("@/components/message", () => ({
  UserMessage: ({ message }: { message: string }) => <p data-testid="user-message">{message}</p>,
  AssistantMessage: ({ message }: { message: string }) => (
    <p data-testid="assistant-message">{message}</p>
  ),
  AssistantTurnFooter: () => null,
}));
vi.mock("@/components/synced-loader", () => ({ SyncedLoader: () => null }));
vi.mock("@/components/ui/icon-button-chrome", () => ({ mutedIconColorMapping: () => ({}) }));
vi.mock("@/components/ui/pane-content-toolbar", () => ({
  ToolbarButton: ({
    label,
    onPress,
    disabled,
    testID,
  }: {
    label: string;
    onPress: () => void;
    disabled?: boolean;
    testID?: string;
  }) => (
    <button data-testid={testID} disabled={disabled} onClick={onPress}>
      {label}
    </button>
  ),
}));
vi.mock("@/composer/dock", () => ({
  ComposerDock: ({ children }: { children: React.ReactNode[] }) => <div>{children}</div>,
}));
vi.mock("@/composer/pill-styles", () => ({ resolveComposerTrackTailClearance: () => 40 }));
vi.mock("@/composer/tracks", () => ({
  ComposerTrackBar: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  ComposerTrackPill: ({ testID, children }: { testID: string; children: React.ReactNode }) => (
    <div data-testid={testID}>{children}</div>
  ),
  ComposerTrackRow: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));
vi.mock("@/constants/layout", () => ({ useIsCompactFormFactor: () => false }));
vi.mock("@/hooks/use-settings", () => ({ useSettings: () => undefined }));
vi.mock("@/panels/pane-context", () => ({ usePaneContext: () => ({ tabId: "bot" }) }));
vi.mock("@/subagents/opens-in-sheet", () => ({ useSubagentOpensInSheet: () => () => false }));
vi.mock("@/workspace-tabs/model", () => ({ buildWorkspaceTabPersistenceKey: () => "workspace" }));
vi.mock("@/workspace-tabs/open-beside", () => ({ openPreferredWorkspaceTarget }));
vi.mock("@/subagents/track", () => ({
  SubagentsTrack: ({
    rows,
    onOpenSubagent,
  }: {
    rows: { id: string; title: string; status: string }[];
    onOpenSubagent: (id: string) => void;
  }) =>
    rows.map((row) => (
      <button
        key={row.id}
        data-testid={`subagent-${row.id}`}
        data-status={row.status}
        onClick={() => onOpenSubagent(row.id)}
      >
        {row.title}
      </button>
    )),
}));
vi.mock("@/composer/input/input", () => ({
  MessageInput: React.forwardRef(function MessageInput(
    {
      onChangeText,
      onSubmit,
      disabled,
      isSubmitDisabled,
      submitButtonTestID,
      leftContent,
      beforeVoiceContent,
      activeActionContent,
    }: {
      onChangeText: (text: string) => void;
      onSubmit: (payload: { text: string; attachments: never[]; cwd: string }) => void;
      disabled?: boolean;
      isSubmitDisabled?: boolean;
      submitButtonTestID: string;
      leftContent: React.ReactNode;
      beforeVoiceContent: React.ReactNode;
      activeActionContent: React.ReactNode;
    },
    ref: React.Ref<{ replaceText: (text: string) => void }>,
  ) {
    const area = React.useRef<HTMLTextAreaElement>(null);
    React.useImperativeHandle(ref, () => ({
      replaceText: (text: string) => {
        if (area.current) area.current.value = text;
      },
    }));
    return (
      <div>
        <textarea
          ref={area}
          data-testid="bot-message-input"
          disabled={disabled}
          onChange={(event) => onChangeText(event.target.value)}
        />
        <button
          data-testid={submitButtonTestID}
          disabled={isSubmitDisabled}
          onClick={() => onSubmit({ text: area.current?.value ?? "", attachments: [], cwd: "/w" })}
        />
        {leftContent}
        {beforeVoiceContent}
        {activeActionContent}
      </div>
    );
  }),
}));

const bot: BotView = {
  workspaceId: "workspace",
  execution: { provider: "codex", model: null, modeId: null, thinking: null },
  builtinChannelId: "22222222-2222-4222-8222-222222222222",
  state: { kind: "active" },
  createdAt: "2026-10-12",
};
const sessionAgentId = "88888888-8888-4888-8888-888888888888";
const conversation = {
  sessionAgentId,
  id: "33333333-3333-4333-8333-333333333333",
  channelId: bot.builtinChannelId,
  peer: "owner",
  sender: "owner",
  thread: null,
  generation: 1,
  nextSequence: 1,
};
function pausedBot(reason: "user" | "directory_missing"): BotView {
  return { ...bot, state: { kind: "paused", reason } };
}
const receipt: BotTurn = {
  id: "44444444-4444-4444-8444-444444444444",
  conversationId: conversation.id,
  messageId: "message",
  sequence: 1,
  generation: 1,
  text: "hello",
  replyTo: null,
  reply: "reply",
  messages: [{ id: "reply", text: "reply", status: "completed" }],
  pendingPermissions: [],
  status: "completed",
  delivery: "delivered",
  error: null,
  createdAt: "2026-10-12",
};
let queryClient: QueryClient;

function renderChat({ bot: view = bot, serverId = "host" }: { bot?: BotView; serverId?: string }) {
  return render(
    <QueryClientProvider client={queryClient}>
      <BotChat serverId={serverId} workspaceId="workspace" cwd="/w" bot={view} interactive />
    </QueryClientProvider>,
  );
}

/** Renders the chat with the Bot read from the query cache, as the Bot tab does. */
function CachedBotChat() {
  const { data } = useQuery<BotView>({
    queryKey: botKeys.bot("host", "workspace"),
    queryFn: () => Promise.reject(new Error("Bot read unexpectedly")),
    staleTime: Infinity,
  });
  return data ? (
    <BotChat serverId="host" workspaceId="workspace" cwd="/w" bot={data} interactive />
  ) : null;
}

function renderCachedBotChat() {
  return render(
    <QueryClientProvider client={queryClient}>
      <CachedBotChat />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  botRequest.mockReset();
  openPreferredWorkspaceTarget.mockReset();
  closeBotSessionTab.mockReset();
  tickets.current = false;
});
afterEach(() => {
  cleanup();
  queryClient.clear();
});

describe("Bot input receipts", () => {
  it("reads a shared Bot conversation without opening it or exposing write actions", async () => {
    botRequest.mockImplementation(async (method: string) => {
      if (method === "bot.conversation.list.request") return { conversations: [conversation] };
      if (method === "bot.history.get.request")
        return { turns: [{ ...receipt, status: "running" }], nextSequence: 1 };
      throw new Error(`Unexpected write: ${method}`);
    });
    render(
      <QueryClientProvider client={queryClient}>
        <BotChat serverId="shared:grant" workspaceId="workspace" cwd="/w" bot={bot} interactive />
      </QueryClientProvider>,
    );

    await screen.findByText("reply");
    expect(screen.queryByTestId("bot-message-input")).toBeNull();
    expect(screen.queryByTestId("bot-message-send")).toBeNull();
    expect(screen.queryByText("bots.cancel")).toBeNull();
    expect(screen.queryByText("bots.reset")).toBeNull();
    expect(botRequest.mock.calls.map(([method]) => method)).toEqual([
      "bot.conversation.list.request",
      "bot.history.get.request",
    ]);
  });

  it("keeps text and reuses message identity after a lost acceptance response", async () => {
    let sends = 0;
    botRequest.mockImplementation(async (method: string) => {
      if (method === "bot.conversation.open.request") return { conversation };
      if (method === "bot.history.get.request") return { turns: [], nextSequence: null };
      if (method === "bot.message.send.request") {
        if (++sends === 1) throw new Error("Response lost");
        return { turn: receipt, accepted: true, duplicate: true };
      }
      throw new Error(`Unexpected ${method}`);
    });
    render(
      <QueryClientProvider client={queryClient}>
        <BotChat serverId="host" workspaceId="workspace" cwd="/w" bot={bot} interactive />
      </QueryClientProvider>,
    );
    const input = screen.getByTestId("bot-message-input") as HTMLTextAreaElement;
    await waitFor(() => expect(input.disabled).toBe(false));
    fireEvent.change(input, { target: { value: "hello" } });
    fireEvent.click(screen.getByTestId("bot-message-send"));
    await screen.findByText("Response lost");
    expect(input.value).toBe("hello");
    fireEvent.click(screen.getByTestId("bot-message-send"));
    await waitFor(() =>
      expect((screen.getByTestId("bot-message-input") as HTMLTextAreaElement).value).toBe(""),
    );
    const sent = botRequest.mock.calls.filter(([method]) => method === "bot.message.send.request");
    expect(sent).toHaveLength(2);
    expect(sent[0][1]).toEqual(sent[1][1]);
    expect(screen.getAllByText("reply")).toHaveLength(1);
  });
  it("reads paused history from an empty cache without opening the conversation", async () => {
    botRequest.mockImplementation(async (method: string) => {
      if (method === "bot.conversation.list.request") return { conversations: [conversation] };
      if (method === "bot.history.get.request") return { turns: [receipt], nextSequence: 1 };
      throw new Error(`Unexpected ${method}`);
    });
    renderChat({ bot: pausedBot("user") });
    await screen.findByText("reply");
    expect((screen.getByTestId("bot-message-input") as HTMLTextAreaElement).disabled).toBe(true);
    expect(botRequest.mock.calls.map(([method]) => method)).toEqual([
      "bot.conversation.list.request",
      "bot.history.get.request",
    ]);
  });

  it.each(["user", "directory_missing"] as const)(
    "resuming a bot paused for %s enables the composer without remounting",
    async (reason) => {
      const resumed: BotView = { ...bot, state: { kind: "active" } };
      botRequest.mockImplementation(async (method: string) => {
        if (method === "bot.conversation.list.request") return { conversations: [conversation] };
        if (method === "bot.conversation.open.request") return { conversation };
        if (method === "bot.history.get.request") return { turns: [receipt], nextSequence: 1 };
        if (method === "bot.resume.request") return { bot: resumed };
        throw new Error(`Unexpected ${method}`);
      });
      queryClient.setQueryData(botKeys.bot("host", "workspace"), pausedBot(reason));
      renderCachedBotChat();
      expect(
        screen.getByText(reason === "user" ? "bots.paused" : "bots.directoryMissing"),
      ).toBeTruthy();
      await screen.findByText("reply");
      expect(screen.getByTestId("bot-message-input")).toHaveProperty("disabled", true);
      expect(botRequest).not.toHaveBeenCalledWith(
        "bot.conversation.open.request",
        expect.anything(),
      );
      fireEvent.click(screen.getByText("bots.resume"));
      await waitFor(() =>
        expect(screen.getByTestId("bot-message-input")).toHaveProperty("disabled", false),
      );
      expect(queryClient.getQueryData(botKeys.bot("host", "workspace"))).toEqual(resumed);
      expect(botRequest).toHaveBeenCalledWith("bot.resume.request", { workspaceId: "workspace" });
      expect(botRequest).toHaveBeenCalledWith("bot.conversation.open.request", {
        workspaceId: "workspace",
      });
      expect(screen.queryByText("bots.resume")).toBeNull();
    },
  );

  it("explains a missing directory and keeps the bot paused when resume fails", async () => {
    botRequest.mockImplementation(async (method: string) => {
      if (method === "bot.resume.request") throw new Error("WorkspaceNotFound");
      if (method === "bot.conversation.list.request") return { conversations: [conversation] };
      if (method === "bot.history.get.request") return { turns: [receipt], nextSequence: 1 };
      throw new Error(`Unexpected ${method}`);
    });
    queryClient.setQueryData(botKeys.bot("host", "workspace"), pausedBot("directory_missing"));
    renderCachedBotChat();
    expect(screen.getByText("bots.directoryMissing")).toBeTruthy();
    await screen.findByText("reply");
    fireEvent.click(screen.getByText("bots.resume"));
    expect(await screen.findByText("WorkspaceNotFound")).toBeTruthy();
    expect(queryClient.getQueryData(botKeys.bot("host", "workspace"))).toEqual(
      pausedBot("directory_missing"),
    );
    expect(screen.getByTestId("bot-resume")).toHaveProperty("disabled", false);
    expect(screen.getByTestId("bot-message-input")).toHaveProperty("disabled", true);
    expect(screen.getByTestId("bot-message-send")).toHaveProperty("disabled", true);
    expect(botRequest).not.toHaveBeenCalledWith("bot.conversation.open.request", expect.anything());
  });

  it("reads a fresh conversation when the Bot is replaced", async () => {
    const replacement: BotView = {
      ...bot,
      builtinChannelId: "66666666-6666-4666-8666-666666666666",
    };
    const replacementConversation = {
      ...conversation,
      id: "77777777-7777-4777-8777-777777777777",
      channelId: replacement.builtinChannelId,
    };
    let current = conversation;
    botRequest.mockImplementation(async (method: string) => {
      if (method === "bot.conversation.open.request") return { conversation: current };
      if (method === "bot.history.get.request") return { turns: [], nextSequence: null };
      throw new Error(`Unexpected ${method}`);
    });
    const { rerender } = renderChat({});
    await waitFor(() =>
      expect(screen.getByTestId("bot-message-input")).toHaveProperty("disabled", false),
    );
    current = replacementConversation;
    rerender(
      <QueryClientProvider client={queryClient}>
        <BotChat serverId="host" workspaceId="workspace" cwd="/w" bot={replacement} interactive />
      </QueryClientProvider>,
    );
    await waitFor(() =>
      expect(
        botRequest.mock.calls.filter(([method]) => method === "bot.conversation.open.request"),
      ).toHaveLength(2),
    );
    await waitFor(() =>
      expect(botRequest).toHaveBeenCalledWith(
        "bot.history.get.request",
        expect.objectContaining({ conversationId: replacementConversation.id }),
      ),
    );
  });

  it("hides resume from read-only users", async () => {
    botRequest.mockImplementation(async (method: string) =>
      method === "bot.conversation.list.request"
        ? { conversations: [conversation] }
        : { turns: [], nextSequence: null },
    );
    renderChat({ bot: pausedBot("user"), serverId: "shared:grant" });
    expect(screen.getByText("bots.paused")).toBeTruthy();
    expect(screen.queryByText("bots.resume")).toBeNull();
  });

  it("sends messages with workspaceId and shows cancellation reasons", async () => {
    const cancelled: BotTurn = {
      ...receipt,
      id: "55555555-5555-4555-8555-555555555555",
      sequence: 0,
      status: "cancelled",
      reply: null,
      messages: [],
      error: "Workspace directory missing; Bot paused",
    };
    botRequest.mockImplementation(async (method: string) => {
      if (method === "bot.conversation.open.request") return { conversation };
      if (method === "bot.history.get.request") return { turns: [cancelled], nextSequence: null };
      if (method === "bot.message.send.request")
        return { turn: receipt, accepted: true, duplicate: false };
      throw new Error(`Unexpected ${method}`);
    });
    renderChat({});
    expect(await screen.findByText("Workspace directory missing; Bot paused")).toBeTruthy();
    const input = screen.getByTestId("bot-message-input") as HTMLTextAreaElement;
    await waitFor(() => expect(input.disabled).toBe(false));
    fireEvent.change(input, { target: { value: "hello" } });
    fireEvent.click(screen.getByTestId("bot-message-send"));
    await waitFor(() =>
      expect(botRequest).toHaveBeenCalledWith("bot.message.send.request", {
        workspaceId: "workspace",
        conversationId: conversation.id,
        messageId: expect.any(String),
        text: "hello",
      }),
    );
  });
});

describe("Bot conversation presentation", () => {
  const withHistory = (turns: BotTurn[]) =>
    botRequest.mockImplementation(async (method: string) => {
      if (method === "bot.conversation.open.request") return { conversation };
      if (method === "bot.history.get.request") return { turns, nextSequence: null };
      if (method === "bot.turn.get.request") return { turn: turns[0] };
      if (method === "bot.turn.cancel.request") return {};
      throw new Error(`Unexpected ${method}`);
    });

  it("shows the prompt and the reply as separate messages", async () => {
    withHistory([receipt]);
    renderChat({});
    expect((await screen.findByTestId("user-message")).textContent).toBe("hello");
    const reply = await screen.findByTestId("assistant-message");
    expect(reply.textContent).toBe("reply");
    expect(screen.getByTestId("user-message").contains(reply)).toBe(false);
  });

  it("keeps distinct assistant messages as separate bubbles", async () => {
    withHistory([
      {
        ...receipt,
        messages: [
          { id: "first", text: "first reply", status: "completed" },
          { id: "second", text: "second reply", status: "completed" },
        ],
        reply: "second reply",
      },
    ]);
    renderChat({});
    const replies = await screen.findAllByTestId("assistant-message");
    expect(replies.map((reply) => reply.textContent)).toEqual(["first reply", "second reply"]);
  });

  it("lists the hidden session as a subagent and opens it as an agent tab", async () => {
    withHistory([{ ...receipt, status: "running" }]);
    renderChat({});
    const row = await screen.findByTestId(`subagent-${sessionAgentId}`);
    expect(row.textContent).toBe("bots.session");
    expect(row.getAttribute("data-status")).toBe("running");
    fireEvent.click(row);
    expect(openPreferredWorkspaceTarget).toHaveBeenCalledWith(
      expect.objectContaining({
        target: { kind: "agent", agentId: sessionAgentId },
        source: "subagents",
      }),
    );
  });

  it("omits the session row until the first turn creates the session", async () => {
    withHistory([]);
    renderChat({});
    await waitFor(() =>
      expect(screen.getByTestId("bot-message-input")).toHaveProperty("disabled", false),
    );
    expect(screen.queryByTestId(`subagent-${sessionAgentId}`)).toBeNull();
  });

  it("omits the session row after a reset until a new turn starts", async () => {
    withHistory([{ ...receipt, generation: 0 }]);
    renderChat({});
    await screen.findByTestId("assistant-message");
    expect(screen.queryByTestId(`subagent-${sessionAgentId}`)).toBeNull();
  });

  it("closes the retired session tab when a reset rotates the session", async () => {
    botRequest.mockImplementation(async (method: string) => {
      if (method === "bot.conversation.open.request") return { conversation };
      if (method === "bot.history.get.request") return { turns: [receipt], nextSequence: null };
      if (method === "bot.conversation.reset.request")
        return { conversation: { ...conversation, generation: 2, sessionAgentId: "next" } };
      throw new Error(`Unexpected ${method}`);
    });
    renderChat({});
    await screen.findByTestId(`subagent-${sessionAgentId}`);
    fireEvent.click(screen.getByTestId("bot-reset"));
    await waitFor(() =>
      expect(closeBotSessionTab).toHaveBeenCalledWith("host", "workspace", sessionAgentId),
    );
    expect(screen.queryByTestId(`subagent-${sessionAgentId}`)).toBeNull();
  });

  it("cancels the running turn from the composer", async () => {
    withHistory([{ ...receipt, status: "running" }]);
    renderChat({});
    fireEvent.click(await screen.findByTestId("bot-cancel"));
    await waitFor(() =>
      expect(botRequest).toHaveBeenCalledWith("bot.turn.cancel.request", {
        workspaceId: "workspace",
        turnId: receipt.id,
      }),
    );
  });
});

describe("Bot wake turns", () => {
  const notification = {
    id: "agent:a1:2026-11-12T00:00:00Z",
    workspaceId: "workspace",
    scope: "workspace",
    kind: "agent_finished",
    agentId: "a1",
    scheduleId: null,
    runId: null,
    terminalId: null,
    title: 'Agent "fix tests" finished',
    summary: "All green",
    at: "2026-11-12T00:00:00Z",
    conversationId: conversation.id,
  };
  const wakeTurn: BotTurn = {
    ...receipt,
    id: "55555555-5555-4555-8555-555555555555",
    messageId: "notification:55555555-5555-4555-8555-555555555555",
    sequence: 2,
    text: "<ait-notifications>machine text</ait-notifications>",
    reply: "Tests pass now.",
    messages: [{ id: "m1", text: "Tests pass now.", status: "completed" }],
    wake: {
      notifications: [
        notification,
        {
          ...notification,
          id: "schedule:s1:r1",
          kind: "schedule_failed",
          agentId: "a2",
          scheduleId: "s1",
          runId: "r1",
          title: 'Schedule "nightly" run failed',
          summary: null,
        },
        {
          ...notification,
          id: "agent:elsewhere:1",
          workspaceId: "other-workspace",
          agentId: "elsewhere",
          title: 'Agent "elsewhere" finished',
          summary: null,
        },
      ],
    },
  };
  function serve(turns: BotTurn[]) {
    botRequest.mockImplementation(async (method: string) => {
      if (method === "bot.conversation.open.request") return { conversation };
      if (method === "bot.history.get.request") return { turns, nextSequence: null };
      if (method === "bot.turn.get.request") return { turn: turns.find(isPendingTurn) };
      throw new Error(`Unexpected ${method}`);
    });
  }
  function isPendingTurn(turn: BotTurn) {
    return turn.status === "running";
  }

  it("shows what woke the Bot instead of a user message and links subjects in this workspace", async () => {
    serve([receipt, wakeTurn]);
    renderChat({});
    await screen.findByTestId(`bot-wake-${wakeTurn.id}`);
    expect(screen.queryByText(wakeTurn.text)).toBeNull();
    // Only the legacy user turn renders a user bubble.
    expect(screen.getAllByTestId("user-message").map((node) => node.textContent)).toEqual([
      "hello",
    ]);
    expect(screen.getByText("All green")).toBeTruthy();
    expect(screen.getByText("Tests pass now.")).toBeTruthy();
    fireEvent.click(screen.getByTestId(`bot-wake-open-${notification.id}`));
    fireEvent.click(screen.getByTestId("bot-wake-open-schedule:s1:r1"));
    expect(openPreferredWorkspaceTarget.mock.calls.map(([input]) => input.target)).toEqual([
      { kind: "agent", agentId: "a1" },
      { kind: "schedule", scheduleId: "s1" },
    ]);
    expect(openPreferredWorkspaceTarget.mock.calls[0][0]).not.toHaveProperty("pin");
    // A subject in another workspace of the Bot's scope stays plain text.
    expect(screen.getByText('Agent "elsewhere" finished')).toBeTruthy();
    expect(screen.queryByTestId("bot-wake-open-agent:elsewhere:1")).toBeNull();
  });

  describe("tasks", () => {
    const ticketId = "66666666-6666-4666-8666-666666666666";
    const reported: BotTurn = {
      ...wakeTurn,
      wake: {
        notifications: [
          {
            ...notification,
            id: `report:${ticketId}:3`,
            kind: "ticket_blocked",
            title: "Fix login",
            summary: "Which provider should log in?",
            ticketId,
            ticketTitle: "Fix login",
          },
          {
            ...notification,
            id: `report:${ticketId}:4`,
            kind: "ticket_done",
            title: "Fix login",
            summary: null,
            ticketId,
            ticketTitle: "Fix login",
          },
          {
            ...notification,
            id: `report:${ticketId}:5`,
            kind: "ticket_failed",
            title: "Fix login",
            summary: null,
          },
        ],
      },
    };
    function serveWithTickets(turns: BotTurn[]) {
      botRequest.mockImplementation(async (method: string) => {
        if (method === "bot.conversation.open.request") return { conversation };
        if (method === "bot.conversation.list.request") return { conversations: [conversation] };
        if (method === "bot.history.get.request") return { turns, nextSequence: null };
        if (method === "bot.ticket.list.request") return { tickets: [] };
        throw new Error(`Unexpected ${method}`);
      });
    }

    it("shows no tasks and sends no ticket request on hosts without tickets", async () => {
      serveWithTickets([reported]);
      renderChat({});
      await screen.findByTestId(`bot-wake-${reported.id}`);
      expect(screen.queryByTestId("bot-tickets")).toBeNull();
      // The task a report names stays plain text with nothing to open.
      const named = screen.getByTestId(`bot-wake-ticket-report:${ticketId}:3`);
      expect(named.textContent).toBe("bots.tickets.linked");
      fireEvent.click(named);
      expect(screen.queryByTestId("bot-ticket-sheet")).toBeNull();
      expect(botRequest.mock.calls.some(([method]) => method.startsWith("bot.ticket."))).toBe(
        false,
      );
    });

    it("labels worker reports and opens the task a wake card names", async () => {
      tickets.current = true;
      serveWithTickets([reported]);
      renderChat({});
      await screen.findByTestId(`bot-wake-${reported.id}`);
      await screen.findByTestId("bot-tickets");
      await waitFor(() =>
        expect(botRequest).toHaveBeenCalledWith("bot.ticket.list.request", {
          workspaceId: "workspace",
          limit: 50,
        }),
      );
      for (const kind of ["ticket_blocked", "ticket_done", "ticket_failed"]) {
        expect(screen.getByLabelText(`bots.notifications.kind.${kind}`)).toBeTruthy();
      }
      expect(screen.queryByTestId(`bot-wake-ticket-report:${ticketId}:5`)).toBeNull();
      fireEvent.click(screen.getByTestId(`bot-wake-ticket-report:${ticketId}:3`));
      const sheet = await screen.findByTestId("bot-ticket-sheet");
      expect(sheet.getAttribute("data-ticket")).toBe(ticketId);
      expect(sheet.getAttribute("data-read-only")).toBe("false");
      // Closing keeps the sheet mounted until its dismissal finishes.
      fireEvent.click(screen.getByTestId("bot-ticket-sheet-close"));
      expect(screen.getByTestId("bot-ticket-sheet").getAttribute("data-visible")).toBe("false");
      fireEvent.click(screen.getByTestId("bot-ticket-sheet-dismiss"));
      expect(screen.queryByTestId("bot-ticket-sheet")).toBeNull();
    });

    it("opens tasks read-only for guests", async () => {
      tickets.current = true;
      serveWithTickets([reported]);
      renderChat({ serverId: "shared:grant" });
      fireEvent.click(await screen.findByTestId(`bot-wake-ticket-report:${ticketId}:4`));
      const sheet = await screen.findByTestId("bot-ticket-sheet");
      expect(sheet.getAttribute("data-read-only")).toBe("true");
    });

    it("hides tasks for a retired Bot", async () => {
      tickets.current = true;
      serveWithTickets([reported]);
      renderChat({ bot: { ...bot, state: { kind: "retired", at: "2026-11-13" } } });
      await screen.findByTestId(`bot-wake-${reported.id}`);
      expect(screen.queryByTestId("bot-tickets")).toBeNull();
      expect(botRequest.mock.calls.some(([method]) => method.startsWith("bot.ticket."))).toBe(
        false,
      );
    });
  });

  it("collapses a silent wake turn to one note without the sentinel", async () => {
    serve([
      {
        ...wakeTurn,
        reply: "NO_REPLY",
        messages: [{ id: "m1", text: " NO_REPLY\n", status: "completed" }],
        silent: true,
      },
    ]);
    renderChat({});
    await screen.findByTestId(`bot-silent-${wakeTurn.id}`);
    expect(screen.getByText("bots.notifications.silent")).toBeTruthy();
    expect(screen.queryByText(/NO_REPLY/)).toBeNull();
    expect(screen.queryAllByTestId("assistant-message")).toHaveLength(0);
  });

  it("never flashes the sentinel while a wake turn streams", async () => {
    const running = (text: string): BotTurn => ({
      ...wakeTurn,
      status: "running",
      delivery: "pending",
      reply: null,
      messages: [{ id: "m1", text, status: "streaming" }],
    });
    serve([running("NO_REP")]);
    renderChat({});
    await screen.findByTestId(`bot-turn-status-${wakeTurn.id}`);
    expect(screen.queryAllByTestId("assistant-message")).toHaveLength(0);
    cleanup();
    queryClient.clear();
    serve([running("Now")]);
    renderChat({});
    expect(await screen.findByText("Now")).toBeTruthy();
  });

  it("keeps a user turn that replies with the sentinel text unchanged", async () => {
    serve([
      {
        ...receipt,
        reply: "NO_REPLY",
        messages: [{ id: "m1", text: "NO_REPLY", status: "completed" }],
      },
    ]);
    renderChat({});
    expect(await screen.findByText("NO_REPLY")).toBeTruthy();
    expect(screen.getByTestId("user-message").textContent).toBe("hello");
    expect(screen.queryByTestId(`bot-silent-${receipt.id}`)).toBeNull();
  });
});

describe("Bot conversations in other channels", () => {
  const telegram = {
    ...conversation,
    id: "66666666-6666-4666-8666-666666666666",
    channelId: "77777777-7777-4777-8777-777777777777",
    channelKind: "telegram",
    peer: "chat-7",
    sender: "alice",
    thread: null,
    sessionAgentId: "99999999-9999-4999-8999-999999999999",
    lastActivityAt: "2026-11-14T00:00:00Z",
  };
  const answered: BotTurn = {
    ...receipt,
    id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    conversationId: telegram.id,
    text: "Is the fix deployed?",
    reply: "Deployed.",
    messages: [{ id: "m1", text: "Deployed.", status: "completed" }],
  };
  const woke: BotTurn = {
    ...answered,
    id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
    messageId: "notification:bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
    sequence: 2,
    text: '<ait-input kind="bot_wake">machine text</ait-input>',
    reply: "NO_REPLY",
    messages: [{ id: "m2", text: "NO_REPLY", status: "completed" }],
    silent: true,
    wake: {
      notifications: [
        {
          id: "agent:a1:1",
          workspaceId: "workspace",
          kind: "agent_finished",
          agentId: "a1",
          title: 'Agent "deploy" finished',
          summary: null,
          at: "2026-11-14T00:00:00Z",
          conversationId: telegram.id,
        },
      ],
    },
  };

  function serve(conversations: (typeof conversation)[]) {
    botRequest.mockImplementation(async (method: string, params: { conversationId?: string }) => {
      if (method === "bot.conversation.open.request") return { conversation };
      if (method === "bot.conversation.list.request") return { conversations };
      if (method === "bot.history.get.request")
        return {
          turns: params.conversationId === telegram.id ? [answered, woke] : [receipt],
          nextSequence: null,
        };
      throw new Error(`Unexpected ${method}`);
    });
  }

  it("shows another channel's conversation read-only, rendered like the built-in one", async () => {
    serve([conversation, telegram]);
    renderChat({});
    const switcher = (await screen.findByTestId("bot-conversation")) as HTMLSelectElement;
    expect([...switcher.options].map((option) => option.textContent)).toEqual([
      "bots.conversationOwner",
      "bots.telegram · chat-7 · alice",
    ]);
    await screen.findByText("reply");
    expect(screen.getByTestId("bot-message-input")).toBeTruthy();

    fireEvent.change(switcher, { target: { value: telegram.id } });

    expect(await screen.findByText("Deployed.")).toBeTruthy();
    expect(screen.getByText("Is the fix deployed?")).toBeTruthy();
    expect(screen.getByTestId(`bot-wake-${woke.id}`)).toBeTruthy();
    expect(screen.getByTestId(`bot-silent-${woke.id}`)).toBeTruthy();
    expect(screen.queryByText("reply")).toBeNull();
    expect(screen.getByTestId("bot-conversation-read-only")).toBeTruthy();
    expect(screen.queryByTestId("bot-message-input")).toBeNull();
    expect(screen.queryByTestId("bot-reset")).toBeNull();
    expect(screen.queryByTestId(`subagent-${telegram.sessionAgentId}`)).toBeNull();
    expect(
      botRequest.mock.calls.some(
        ([method, params]) =>
          method === "bot.history.get.request" && params.conversationId === telegram.id,
      ),
    ).toBe(true);
    expect(botRequest.mock.calls.map(([method]) => method)).not.toContain(
      "bot.message.send.request",
    );

    // Choosing the built-in conversation again restores the composer.
    fireEvent.change(switcher, { target: { value: "" } });
    expect(await screen.findByText("reply")).toBeTruthy();
    expect(screen.getByTestId("bot-message-input")).toBeTruthy();
    expect(screen.queryByTestId("bot-conversation-read-only")).toBeNull();
  });

  it("offers no switch while the Bot has only its built-in conversation", async () => {
    serve([conversation]);
    renderChat({});
    await screen.findByText("reply");
    await waitFor(() =>
      expect(botRequest.mock.calls.map(([method]) => method)).toContain(
        "bot.conversation.list.request",
      ),
    );
    expect(screen.queryByTestId("bot-conversation")).toBeNull();
  });

  it("lets a guest read other conversations too, without writing", async () => {
    serve([conversation, telegram]);
    render(
      <QueryClientProvider client={queryClient}>
        <BotChat serverId="shared:grant" workspaceId="workspace" cwd="/w" bot={bot} interactive />
      </QueryClientProvider>,
    );
    const switcher = (await screen.findByTestId("bot-conversation")) as HTMLSelectElement;
    fireEvent.change(switcher, { target: { value: telegram.id } });
    expect(await screen.findByText("Deployed.")).toBeTruthy();
    expect(screen.queryByTestId("bot-message-input")).toBeNull();
    expect(botRequest.mock.calls.map(([method]) => method)).not.toContain(
      "bot.conversation.open.request",
    );
  });
});

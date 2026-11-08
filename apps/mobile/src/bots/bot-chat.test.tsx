// @vitest-environment jsdom
import React from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BotTurn, BotView } from "@ait/protocol/bot/types";
import { BotChat } from "./bot-chat";
import { botKeys } from "./queries";

const { botRequest, closeBotSessionTab, openPreferredWorkspaceTarget } = vi.hoisted(() => ({
  botRequest: vi.fn(),
  closeBotSessionTab: vi.fn(),
  openPreferredWorkspaceTarget: vi.fn(),
}));
vi.mock("@/runtime/host-runtime", () => ({ useHostRuntimeClient: () => ({ botRequest }) }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("./styles", () => ({ botStyles: {}, botChatStyles: {} }));
vi.mock("./loading", () => ({ BotLoadingSpinner: () => null }));
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

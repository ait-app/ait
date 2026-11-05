// @vitest-environment jsdom
import React from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BotTurn, BotView } from "@ait/protocol/bot/types";
import { BotChat } from "./bot-chat";
import { botKeys } from "./queries";

const { botRequest } = vi.hoisted(() => ({ botRequest: vi.fn() }));
vi.mock("@/runtime/host-runtime", () => ({ useHostRuntimeClient: () => ({ botRequest }) }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("./styles", () => ({ botStyles: {} }));
vi.mock("./loading", () => ({ BotLoadingSpinner: () => null }));
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
vi.mock("@/components/ui/form-field", () => ({
  FormTextInput: ({
    initialValue,
    resetKey,
    onChangeText,
    editable,
    testID,
  }: {
    initialValue: string;
    resetKey: number;
    onChangeText: (text: string) => void;
    editable: boolean;
    testID: string;
  }) => (
    <textarea
      key={resetKey}
      data-testid={testID}
      defaultValue={initialValue}
      disabled={!editable}
      onChange={(event) => onChangeText(event.target.value)}
    />
  ),
}));

const bot: BotView = {
  workspaceId: "workspace",
  execution: { provider: "codex", model: null, modeId: null, thinking: null },
  builtinChannelId: "22222222-2222-4222-8222-222222222222",
  state: { kind: "active" },
  createdAt: "2026-10-12",
};
const conversation = {
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
      <BotChat serverId={serverId} workspaceId="workspace" bot={view} interactive />
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
  return data ? <BotChat serverId="host" workspaceId="workspace" bot={data} interactive /> : null;
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
        <BotChat serverId="shared:grant" workspaceId="workspace" bot={bot} interactive />
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
        <BotChat serverId="host" workspaceId="workspace" bot={bot} interactive />
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
        <BotChat serverId="host" workspaceId="workspace" bot={replacement} interactive />
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

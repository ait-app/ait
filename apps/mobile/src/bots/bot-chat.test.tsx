// @vitest-environment jsdom
import React from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Bot, BotTurn } from "@ait/protocol/bot/types";
import { BotChat } from "./bot-chat";

const { botRequest } = vi.hoisted(() => ({ botRequest: vi.fn() }));
vi.mock("@/runtime/host-runtime", () => ({ useHostRuntimeClient: () => ({ botRequest }) }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("./styles", () => ({ botStyles: {} }));
vi.mock("./loading", () => ({ BotLoadingSpinner: () => null }));
vi.mock("./bot-permission", () => ({ BotPermission: () => null }));
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

const bot: Bot = {
  id: "11111111-1111-4111-8111-111111111111",
  workspaceId: "workspace",
  name: "Helper",
  instructions: "",
  execution: { provider: "codex", model: null, modeId: null, thinking: null },
  builtinChannelId: "22222222-2222-4222-8222-222222222222",
  enabled: true,
  archivedAt: null,
  createdAt: "2026-10-12",
};
const conversation = {
  id: "33333333-3333-4333-8333-333333333333",
  botId: bot.id,
  channelId: bot.builtinChannelId,
  peer: "owner",
  sender: "owner",
  thread: null,
  generation: 1,
  nextSequence: 1,
};
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
        <BotChat serverId="host" bot={bot} interactive />
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
  it("allows history reads while paused and prevents new input", async () => {
    botRequest.mockImplementation(async (method: string) =>
      method === "bot.conversation.open.request"
        ? { conversation }
        : { turns: [receipt], nextSequence: 1 },
    );
    render(
      <QueryClientProvider client={queryClient}>
        <BotChat serverId="host" bot={{ ...bot, enabled: false }} interactive />
      </QueryClientProvider>,
    );
    await screen.findByText("reply");
    expect((screen.getByTestId("bot-message-input") as HTMLTextAreaElement).disabled).toBe(true);
    expect(botRequest.mock.calls.some(([method]) => method === "bot.message.send.request")).toBe(
      false,
    );
  });
});

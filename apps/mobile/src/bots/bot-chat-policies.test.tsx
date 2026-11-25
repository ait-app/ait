// @vitest-environment jsdom
import React from "react";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { BotChat } from "@ait/protocol/bot/types";
import { SettingsCard } from "@/components/settings";
import { BotChatPolicyRow, useBotChats } from "./bot-chat-policies";
import { botKeys } from "./queries";

const { botRequest } = vi.hoisted(() => ({ botRequest: vi.fn() }));
vi.mock("@/runtime/host-runtime", () => ({ useHostRuntimeClient: () => ({ botRequest }) }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: { platform?: string }) =>
      `${key}${values?.platform ? `:${values.platform}` : ""}`,
  }),
}));
// Keep the settings row/card/select implementation; only the portal/menu transport is simplified.
vi.mock("@/components/ui/dropdown-menu", () => ({
  DropdownMenu: ({ children }: React.PropsWithChildren) => <>{children}</>,
  DropdownMenuContent: ({ children }: React.PropsWithChildren) => <>{children}</>,
  DropdownMenuItem: ({ children, onSelect }: React.PropsWithChildren<{ onSelect: () => void }>) => (
    <button onClick={onSelect}>{children}</button>
  ),
}));
vi.mock("@/components/ui/dropdown-trigger", () => ({
  DropdownTrigger: ({
    children,
    disabled,
    testID,
    accessibilityLabel,
  }: React.PropsWithChildren<{
    disabled: boolean;
    testID: string;
    accessibilityLabel: string;
  }>) => (
    <button disabled={disabled} data-testid={testID} aria-label={accessibilityLabel}>
      {children}
    </button>
  ),
}));

const chat: BotChat = {
  channelId: "c",
  peer: "group-a",
  group: true,
  enabled: true,
  listenAllMessages: false,
};
let cache: QueryClient;
function Policies({
  interactive = true,
  disabled = false,
}: {
  interactive?: boolean;
  disabled?: boolean;
}) {
  const chats = useBotChats("s", "w", "b", interactive);
  return (
    <SettingsCard>
      {chats.data?.map((item) => (
        <BotChatPolicyRow
          key={item.peer}
          serverId="s"
          workspaceId="w"
          builtinChannelId="b"
          chat={item}
          platform="Lark"
          disabled={disabled || !interactive}
        />
      ))}
    </SettingsCard>
  );
}
function content(props: React.ComponentProps<typeof Policies> = {}) {
  return (
    <QueryClientProvider client={cache}>
      <Policies {...props} />
    </QueryClientProvider>
  );
}
const row = (peer = "group-a") => screen.getByTestId(`bot-chat-policy-c-${peer}`);
beforeEach(() => {
  cache = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  botRequest.mockReset().mockImplementation(async (method: string, params: BotChat) =>
    method === "bot.chat.list.request"
      ? {
          chats: [chat, { ...chat, peer: "group-b" }, { ...chat, peer: "private", group: false }],
        }
      : { chat: { ...chat, ...params } },
  );
});
afterEach(() => {
  cleanup();
  cache.clear();
  vi.useRealTimers();
});

it("renders one native settings row per chat and only two modes for direct messages", async () => {
  render(content());
  await screen.findByTestId("bot-chat-policy-c-private");
  expect(within(row()).getByText("bots.groupChat · group-a")).toBeTruthy();
  expect(within(row("private")).queryByText("bots.receptionMentions")).toBeNull();
  expect(within(row("private")).queryByText("bots.receptionAll")).toBeNull();
  expect(
    within(row("private")).getByText("bots.receptionDirect", { selector: "button" }),
  ).toBeTruthy();
});
it("saves only the selected row, keeps other rows editable, and uses the response cache", async () => {
  let finish!: (value: { chat: BotChat }) => void;
  const original = botRequest.getMockImplementation()!;
  botRequest.mockImplementation((method: string, params: BotChat) =>
    method === "bot.chat.update.request"
      ? new Promise((resolve) => {
          finish = resolve;
        })
      : original(method, params),
  );
  render(content());
  await screen.findByTestId("bot-chat-policy-c-group-a");
  fireEvent.click(within(row()).getByText("bots.receptionAll", { selector: "button" }));
  await waitFor(() =>
    expect(screen.getByTestId("bot-chat-policy-c-group-a-trigger").hasAttribute("disabled")).toBe(
      true,
    ),
  );
  expect(screen.getByTestId("bot-chat-policy-c-group-b-trigger").hasAttribute("disabled")).toBe(
    false,
  );
  expect(botRequest).toHaveBeenCalledWith("bot.chat.update.request", {
    workspaceId: "w",
    channelId: "c",
    peer: "group-a",
    enabled: true,
    listenAllMessages: true,
  });
  await act(async () => finish({ chat: { ...chat, listenAllMessages: true } }));
  expect(await within(row()).findByText("bots.listenAllPermissions:Lark")).toBeTruthy();
  expect(cache.getQueryData<BotChat[]>(botKeys.chats("s", "w", "b"))?.[0]?.listenAllMessages).toBe(
    true,
  );
  expect(
    botRequest.mock.calls.filter(([method]) => method === "bot.chat.list.request"),
  ).toHaveLength(1);
});
it("disables reception in one action and preserves its saved all-message setting", async () => {
  botRequest.mockResolvedValueOnce({ chats: [{ ...chat, listenAllMessages: true }] });
  render(content());
  await screen.findByTestId("bot-chat-policy-c-group-a");
  fireEvent.click(within(row()).getByText("bots.receptionOff", { selector: "button" }));
  await waitFor(() =>
    expect(botRequest).toHaveBeenCalledWith("bot.chat.update.request", {
      workspaceId: "w",
      channelId: "c",
      peer: "group-a",
      enabled: false,
      listenAllMessages: true,
    }),
  );
  expect(await within(row()).findByText("bots.chatDisabledHint")).toBeTruthy();
});
it("puts save errors in the native row without altering the stored mode", async () => {
  const original = botRequest.getMockImplementation()!;
  botRequest.mockImplementation((method: string, params: BotChat) =>
    method === "bot.chat.update.request"
      ? Promise.reject(new Error("Save failed"))
      : original(method, params),
  );
  render(content());
  await screen.findByTestId("bot-chat-policy-c-group-a");
  fireEvent.click(within(row()).getByText("bots.receptionAll", { selector: "button" }));
  expect(await within(row()).findByRole("alert")).toHaveProperty("textContent", "Save failed");
  expect(cache.getQueryData<BotChat[]>(botKeys.chats("s", "w", "b"))?.[0]?.listenAllMessages).toBe(
    false,
  );
});
it("gates paused/read-only controls", async () => {
  render(content({ disabled: true }));
  expect(
    (await screen.findByTestId("bot-chat-policy-c-group-a-trigger")).hasAttribute("disabled"),
  ).toBe(true);
});
it("stops polling when not interactive", async () => {
  const view = render(content());
  await screen.findByTestId("bot-chat-policy-c-group-a");
  vi.useFakeTimers();
  view.rerender(content({ interactive: false }));
  botRequest.mockClear();
  await act(async () => vi.advanceTimersByTimeAsync(30_000)); // virtual time
  expect(botRequest).not.toHaveBeenCalled();
});

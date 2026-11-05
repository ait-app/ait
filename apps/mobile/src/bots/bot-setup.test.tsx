// @vitest-environment jsdom
import React from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, expect, it, vi } from "vitest";
import type { BotView } from "@ait/protocol/bot/types";
import { BotSetup } from "./bot-setup";
import { botKeys } from "./queries";

const { botRequest } = vi.hoisted(() => ({ botRequest: vi.fn() }));
vi.mock("@/runtime/host-runtime", () => ({ useHostRuntimeClient: () => ({ botRequest }) }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("./styles", () => ({ botStyles: {} }));
vi.mock("@/hooks/use-providers-snapshot", () => ({
  useProvidersSnapshot: () => ({
    entries: [
      { provider: "claude", label: "Claude", enabled: true, status: "ready", models: [] },
      { provider: "codex", label: "Codex", enabled: true, status: "ready", models: [] },
    ],
    error: null,
  }),
}));
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

const bot: BotView = {
  workspaceId: "workspace",
  execution: { provider: "codex", model: null, modeId: null, thinking: null },
  builtinChannelId: "22222222-2222-4222-8222-222222222222",
  state: { kind: "active" },
  createdAt: "2026-11-06",
};

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

function renderSetup(onConfigured = vi.fn()) {
  const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <BotSetup
        serverId="server"
        workspaceId="workspace"
        cwd="/repo"
        interactive
        onConfigured={onConfigured}
      />
    </QueryClientProvider>,
  );
  return { queryClient, onConfigured };
}

it("setup_sends_clear_memory_only_when_checked", async () => {
  botRequest.mockResolvedValue({ bot, created: true });
  const { queryClient, onConfigured } = renderSetup();
  expect(screen.getByTestId("bot-setup")).toBeTruthy();

  fireEvent.change(screen.getByTestId("bot-provider"), { target: { value: "codex" } });
  fireEvent.click(screen.getByTestId("bot-setup-submit"));
  await waitFor(() => expect(botRequest).toHaveBeenCalledTimes(1));
  const [firstType, firstParams] = botRequest.mock.calls[0];
  expect(firstType).toBe("bot.setup.request");
  expect(firstParams).toMatchObject({
    workspaceId: "workspace",
    execution: { provider: "codex", model: null, modeId: null, thinking: null },
  });
  expect(firstParams.clearMemory ?? false).toBe(false);
  expect(firstParams).not.toHaveProperty("botId");
  await waitFor(() => expect(onConfigured).toHaveBeenCalledWith(bot));
  expect(queryClient.getQueryData(botKeys.bot("server", "workspace"))).toEqual(bot);

  fireEvent.click(screen.getByTestId("bot-clear-memory"));
  fireEvent.click(screen.getByTestId("bot-setup-submit"));
  await waitFor(() => expect(botRequest).toHaveBeenCalledTimes(2));
  const [, secondParams] = botRequest.mock.calls[1];
  expect(secondParams).toMatchObject({ workspaceId: "workspace", clearMemory: true });
  expect(secondParams).not.toHaveProperty("botId");
});

it("setup_drops_conversation_memory_and_channels_cached_for_a_removed_bot", async () => {
  botRequest.mockResolvedValue({ bot, created: true });
  const { queryClient, onConfigured } = renderSetup();
  const stale = [
    botKeys.conversation("server", "workspace"),
    botKeys.memory("server", "workspace"),
    botKeys.channels("server", "workspace"),
  ];
  for (const key of stale) queryClient.setQueryData(key, { stale: true });

  fireEvent.click(screen.getByTestId("bot-setup-submit"));
  await waitFor(() => expect(onConfigured).toHaveBeenCalledWith(bot));
  for (const key of stale) expect(queryClient.getQueryData(key)).toBeUndefined();
});

it("defaults to the first ready provider and leaves clear memory unchecked", () => {
  renderSetup();
  expect((screen.getByTestId("bot-provider") as HTMLSelectElement).value).toBe("claude");
  expect(screen.getByTestId("bot-clear-memory").getAttribute("aria-checked")).toBe("false");
});

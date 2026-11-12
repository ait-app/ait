// @vitest-environment jsdom
import React from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { BotView } from "@ait/protocol/bot/types";
import { BotSetup } from "./bot-setup";
import { botKeys } from "./queries";

const { botRequest, supported, notifications, snapshotEntries } = vi.hoisted(() => ({
  botRequest: vi.fn(),
  supported: { current: false },
  notifications: { current: false },
  snapshotEntries: { current: [] as unknown[] },
}));
vi.mock("@/runtime/host-runtime", () => ({ useHostRuntimeClient: () => ({ botRequest }) }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("./styles", () => ({ botStyles: {} }));
vi.mock("@/stores/session-store", () => ({
  useSessionStore: (select: (state: unknown) => unknown) =>
    select({
      sessions: {
        server: {
          serverInfo: {
            features: {
              workspaceBotPermissions: supported.current,
              workspaceBotNotifications: notifications.current,
            },
          },
        },
      },
    }),
}));
vi.mock("@/hooks/use-providers-snapshot", () => ({
  useProvidersSnapshot: () => ({ entries: snapshotEntries.current, error: null }),
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

const claudeModes = [
  { id: "default", label: "Default", colorTier: "safe" },
  { id: "plan", label: "Plan", colorTier: "planning" },
  { id: "bypassPermissions", label: "Bypass", colorTier: "dangerous" },
];
const fullSupport = {
  instructions: "system_prompt",
  readOnlyMode: "read-only",
  toolPreapproval: true,
  aitTools: true,
};
const providerEntries = () => [
  {
    provider: "claude",
    label: "Claude",
    enabled: true,
    status: "ready",
    models: [],
    modes: claudeModes,
    defaultModeId: "default",
  },
  {
    provider: "codex",
    label: "Codex",
    enabled: true,
    status: "ready",
    models: [],
    modes: [{ id: "read-only", label: "Read only", colorTier: "planning" }],
    defaultModeId: "read-only",
    botSession: fullSupport,
  },
  { provider: "minimax", label: "MiniMax", enabled: true, status: "ready", models: [] },
  { provider: "off", label: "Off", enabled: true, status: "unavailable", models: [] },
];

beforeEach(() => {
  supported.current = false;
  notifications.current = false;
  snapshotEntries.current = providerEntries();
});

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

it("setup_drops_cached_memory_only_when_clearing_it", async () => {
  botRequest.mockResolvedValue({ bot, created: true });
  const { queryClient, onConfigured } = renderSetup();
  const memoryKey = botKeys.memory("server", "workspace");
  queryClient.setQueryData(memoryKey, { text: "kept", revision: "r1" });

  fireEvent.click(screen.getByTestId("bot-setup-submit"));
  await waitFor(() => expect(onConfigured).toHaveBeenCalledTimes(1));
  expect(queryClient.getQueryData(memoryKey)).toEqual({ text: "kept", revision: "r1" });

  fireEvent.click(screen.getByTestId("bot-clear-memory"));
  fireEvent.click(screen.getByTestId("bot-setup-submit"));
  await waitFor(() => expect(onConfigured).toHaveBeenCalledTimes(2));
  expect(queryClient.getQueryData(memoryKey)).toBeUndefined();
});

it("setup_conflict_refreshes_the_existing_bot", async () => {
  botRequest.mockRejectedValue(Object.assign(new Error("Bot conflict"), { code: "bot_conflict" }));
  const { queryClient, onConfigured } = renderSetup();
  queryClient.setQueryData(botKeys.bot("server", "workspace"), null);

  fireEvent.click(screen.getByTestId("bot-setup-submit"));
  expect(await screen.findByText("Bot conflict")).toBeTruthy();
  expect(queryClient.getQueryState(botKeys.bot("server", "workspace"))?.isInvalidated).toBe(true);
  expect(onConfigured).not.toHaveBeenCalled();
});

it("setup_other_errors_leave_the_bot_query_alone", async () => {
  botRequest.mockRejectedValue(new Error("offline"));
  const { queryClient } = renderSetup();
  queryClient.setQueryData(botKeys.bot("server", "workspace"), null);

  fireEvent.click(screen.getByTestId("bot-setup-submit"));
  expect(await screen.findByText("offline")).toBeTruthy();
  expect(queryClient.getQueryState(botKeys.bot("server", "workspace"))?.isInvalidated).toBe(false);
});

it("defaults to the first ready provider and leaves clear memory unchecked", () => {
  renderSetup();
  expect((screen.getByTestId("bot-provider") as HTMLSelectElement).value).toBe("claude");
  expect(screen.getByTestId("bot-clear-memory").getAttribute("aria-checked")).toBe("false");
});

it("setup_sends_default_permissions_when_supported", async () => {
  supported.current = true;
  botRequest.mockResolvedValue({ bot, created: true });
  renderSetup();
  const providerOptions = Array.from(
    (screen.getByTestId("bot-provider") as HTMLSelectElement).options,
  ).map((option) => option.value);
  expect(providerOptions).toEqual(["claude", "codex", "minimax"]);
  expect(screen.getByText("bots.modeCapWarning")).toBeTruthy();
  expect(screen.getByText("bots.commandReachWarning")).toBeTruthy();
  expect(screen.queryByTestId("bot-mode")).toBeNull();
  expect(screen.getByText("bots.readOnlyHint")).toBeTruthy();

  fireEvent.click(screen.getByTestId("bot-setup-submit"));
  await waitFor(() => expect(botRequest).toHaveBeenCalledTimes(1));
  const [, params] = botRequest.mock.calls[0];
  expect(params.execution.modeId).toBeNull();
  expect(params.permissions).toEqual({
    groups: ["observe", "agents", "stop", "schedules"],
    modes: { claude: ["default"], codex: ["read-only"] },
  });
});

it("setup_sends_edited_permissions", async () => {
  supported.current = true;
  botRequest.mockResolvedValue({ bot, created: true });
  renderSetup();
  fireEvent.click(screen.getByTestId("bot-group-schedules"));
  fireEvent.change(screen.getByTestId("bot-mode-claude"), { target: { value: "plan" } });
  fireEvent.click(screen.getByTestId("bot-setup-submit"));
  await waitFor(() => expect(botRequest).toHaveBeenCalledTimes(1));
  expect(botRequest.mock.calls[0][1].permissions).toEqual({
    groups: ["observe", "agents", "stop"],
    modes: { claude: ["plan"], codex: ["read-only"] },
  });
});

it("setup_omits_permissions_on_old_hosts", async () => {
  botRequest.mockResolvedValue({ bot, created: true });
  renderSetup();
  expect(screen.queryByTestId("bot-permissions")).toBeNull();
  expect(screen.queryByText("bots.modeCapWarning")).toBeNull();
  fireEvent.click(screen.getByTestId("bot-setup-submit"));
  await waitFor(() => expect(botRequest).toHaveBeenCalledTimes(1));
  expect(botRequest.mock.calls[0][1]).not.toHaveProperty("permissions");
});

it("setup_keeps_the_mode_picker_on_old_hosts", async () => {
  botRequest.mockResolvedValue({ bot, created: true });
  renderSetup();
  expect(screen.queryByText("bots.readOnlyHint")).toBeNull();
  fireEvent.change(screen.getByTestId("bot-mode"), { target: { value: "plan" } });
  fireEvent.click(screen.getByTestId("bot-setup-submit"));
  await waitFor(() => expect(botRequest).toHaveBeenCalledTimes(1));
  expect(botRequest.mock.calls[0][1].execution.modeId).toBe("plan");
});

it("degraded_bot_provider_shows_capability_notes", () => {
  snapshotEntries.current = [
    ...providerEntries(),
    {
      provider: "acp",
      label: "ACP",
      enabled: true,
      status: "ready",
      models: [],
      botSession: {
        instructions: "turn_prefix",
        readOnlyMode: null,
        toolPreapproval: false,
        aitTools: true,
      },
    },
  ];
  renderSetup();
  const notes = () =>
    screen.queryAllByTestId(/^bot-note-/).map((node) => node.getAttribute("data-testid"));
  fireEvent.change(screen.getByTestId("bot-provider"), { target: { value: "acp" } });
  expect(notes()).toEqual([
    "bot-note-instructionsInInput",
    "bot-note-noReadOnlyMode",
    "bot-note-toolsNeedApproval",
  ]);
  expect(screen.getByText("bots.noteNoReadOnlyMode")).toBeTruthy();
  fireEvent.change(screen.getByTestId("bot-provider"), { target: { value: "codex" } });
  expect(notes()).toEqual([]);
  fireEvent.change(screen.getByTestId("bot-provider"), { target: { value: "minimax" } });
  expect(notes()).toEqual([]);
});

it("setup_omits_wake_scope_on_old_hosts", async () => {
  botRequest.mockResolvedValue({ bot, created: true });
  renderSetup();
  expect(screen.queryByTestId("bot-wake-scope")).toBeNull();
  fireEvent.click(screen.getByTestId("bot-setup-submit"));
  await waitFor(() => expect(botRequest).toHaveBeenCalledTimes(1));
  expect(botRequest.mock.calls[0][1]).not.toHaveProperty("wakeScope");
});

it("setup_sends_the_chosen_wake_scope", async () => {
  notifications.current = true;
  botRequest.mockResolvedValue({ bot: { ...bot, wakeScope: "all" }, created: true });
  renderSetup();
  const select = screen.getByTestId("bot-wake-scope") as HTMLSelectElement;
  expect(select.value).toBe("own");
  expect(screen.getByText("bots.notifications.wakeScopeHint")).toBeTruthy();
  fireEvent.change(select, { target: { value: "all" } });
  fireEvent.click(screen.getByTestId("bot-setup-submit"));
  await waitFor(() => expect(botRequest).toHaveBeenCalledTimes(1));
  expect(botRequest.mock.calls[0][1]).toMatchObject({ wakeScope: "all" });
});

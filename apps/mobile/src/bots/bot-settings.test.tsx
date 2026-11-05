// @vitest-environment jsdom
import React from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BotView } from "@ait/protocol/bot/types";
import { BotSettings } from "./bot-settings";
import { botKeys } from "./queries";

const { botRequest, canManage, confirmDialog } = vi.hoisted(() => ({
  botRequest: vi.fn(),
  canManage: vi.fn(() => true),
  confirmDialog: vi.fn(async (_options: unknown) => true),
}));
vi.mock("@/runtime/host-runtime", () => ({ useHostRuntimeClient: () => ({ botRequest }) }));
vi.mock("@/hosts/background-task-access", () => ({ canManageBackgroundTasks: canManage }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("./styles", () => ({ botStyles: {} }));
vi.mock("./loading", () => ({ BotLoadingSpinner: () => null }));
vi.mock("@/utils/confirm-dialog", () => ({ confirmDialog }));
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
    disabled,
    testID,
  }: {
    value: string;
    options: { id: string; label: string }[];
    onChange: (value: string) => void;
    disabled?: boolean;
    testID: string;
  }) => (
    <select
      data-testid={testID}
      value={value}
      disabled={disabled}
      onChange={(event) => onChange(event.target.value)}
    >
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
vi.mock("@/components/ui/form-field", () => ({
  Field: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
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
let queryClient: QueryClient;
const onRemoved = vi.fn();

function renderSettings(view: BotView = bot) {
  return render(
    <QueryClientProvider client={queryClient}>
      <BotSettings
        serverId="host"
        workspaceId="workspace"
        cwd="/repo"
        bot={view}
        interactive
        onRemoved={onRemoved}
      />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  botRequest.mockReset();
  botRequest.mockImplementation(async (method: string) => {
    if (method === "bot.memory.get.request") return { text: "memo", revision: "r1" };
    throw new Error(`unexpected ${method}`);
  });
  canManage.mockReturnValue(true);
  confirmDialog.mockClear();
  onRemoved.mockClear();
});
afterEach(() => {
  cleanup();
  queryClient.clear();
});

describe("BotSettings", () => {
  it("updates execution with workspaceId", async () => {
    const updated = { ...bot, execution: { ...bot.execution, provider: "claude" } };
    botRequest.mockImplementation(async (method: string) => {
      if (method === "bot.memory.get.request") return { text: "memo", revision: "r1" };
      if (method === "bot.update.request")
        return { bot: updated, configurationAppliesToNewSessions: true };
      throw new Error(`unexpected ${method}`);
    });
    renderSettings();
    fireEvent.change(screen.getByTestId("bot-provider"), { target: { value: "claude" } });
    fireEvent.click(screen.getByTestId("bot-settings-save"));
    await waitFor(() =>
      expect(botRequest).toHaveBeenCalledWith("bot.update.request", {
        workspaceId: "workspace",
        execution: { provider: "claude", model: null, modeId: null, thinking: null },
      }),
    );
    await waitFor(() =>
      expect(queryClient.getQueryData(botKeys.bot("host", "workspace"))).toEqual(updated),
    );
  });

  it("pauses an active bot and resumes a paused bot", async () => {
    const paused: BotView = { ...bot, state: { kind: "paused", reason: "user" } };
    botRequest.mockImplementation(async (method: string) => {
      if (method === "bot.memory.get.request") return { text: "memo", revision: "r1" };
      if (method === "bot.pause.request") return { bot: paused };
      if (method === "bot.resume.request") return { bot };
      throw new Error(`unexpected ${method}`);
    });
    renderSettings();
    expect(screen.queryByText("bots.resume")).toBeNull();
    fireEvent.click(screen.getByText("bots.pause"));
    await waitFor(() =>
      expect(botRequest).toHaveBeenCalledWith("bot.pause.request", { workspaceId: "workspace" }),
    );
    await waitFor(() =>
      expect(queryClient.getQueryData(botKeys.bot("host", "workspace"))).toEqual(paused),
    );
    cleanup();
    renderSettings(paused);
    expect(screen.queryByText("bots.pause")).toBeNull();
    fireEvent.click(screen.getByText("bots.resume"));
    await waitFor(() =>
      expect(botRequest).toHaveBeenCalledWith("bot.resume.request", { workspaceId: "workspace" }),
    );
  });

  it("remove_returns_to_wizard", async () => {
    botRequest.mockImplementation(async (method: string) => {
      if (method === "bot.memory.get.request") return { text: "memo", revision: "r1" };
      if (method === "bot.remove.request") return { bot: { ...bot, state: { kind: "retired" } } };
      throw new Error(`unexpected ${method}`);
    });
    queryClient.setQueryData(botKeys.bot("host", "workspace"), bot);
    renderSettings();
    fireEvent.click(screen.getByText("bots.remove"));
    await waitFor(() => expect(onRemoved).toHaveBeenCalledTimes(1));
    expect(confirmDialog).toHaveBeenCalledWith(
      expect.objectContaining({ message: "bots.removeConfirm", destructive: true }),
    );
    expect(botRequest).toHaveBeenCalledWith("bot.remove.request", { workspaceId: "workspace" });
    expect(queryClient.getQueryData(botKeys.bot("host", "workspace"))).toBeNull();
  });

  it("does not remove when the confirmation is declined", async () => {
    confirmDialog.mockResolvedValueOnce(false);
    renderSettings();
    fireEvent.click(screen.getByText("bots.remove"));
    await waitFor(() => expect(confirmDialog).toHaveBeenCalled());
    expect(botRequest).not.toHaveBeenCalledWith("bot.remove.request", expect.anything());
    expect(onRemoved).not.toHaveBeenCalled();
  });

  it("saves memory against workspaceId", async () => {
    botRequest.mockImplementation(async (method: string) => {
      if (method === "bot.memory.get.request") return { text: "memo", revision: "r1" };
      if (method === "bot.memory.update.request") return { revision: "r2" };
      throw new Error(`unexpected ${method}`);
    });
    renderSettings();
    fireEvent.change(await screen.findByTestId("bot-memory"), { target: { value: "new" } });
    fireEvent.click(screen.getByTestId("bot-memory-save"));
    await waitFor(() =>
      expect(botRequest).toHaveBeenCalledWith("bot.memory.update.request", {
        workspaceId: "workspace",
        text: "new",
        expected: "r1",
      }),
    );
  });

  it("hides pause and remove and disables edits for read-only users", async () => {
    canManage.mockReturnValue(false);
    renderSettings();
    expect(screen.queryByText("bots.pause")).toBeNull();
    expect(screen.queryByText("bots.remove")).toBeNull();
    expect((screen.getByTestId("bot-provider") as HTMLSelectElement).disabled).toBe(true);
    expect((screen.getByTestId("bot-settings-save") as HTMLButtonElement).disabled).toBe(true);
    expect((await screen.findByTestId("bot-memory")) as HTMLTextAreaElement).toHaveProperty(
      "disabled",
      true,
    );
    expect((screen.getByTestId("bot-memory-save") as HTMLButtonElement).disabled).toBe(true);
  });
});

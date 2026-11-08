// @vitest-environment jsdom
import React from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BotView } from "@ait/protocol/bot/types";
import { BotSettings } from "./bot-settings";
import { botKeys } from "./queries";

const { botRequest, canManage, confirmDialog, supported } = vi.hoisted(() => ({
  supported: { current: true },
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
vi.mock("@/stores/session-store", () => ({
  useSessionStore: (select: (state: unknown) => unknown) =>
    select({
      sessions: {
        host: { serverInfo: { features: { workspaceBotPermissions: supported.current } } },
      },
    }),
}));
vi.mock("@/hooks/use-providers-snapshot", () => ({
  useProvidersSnapshot: () => ({
    entries: [
      { provider: "claude", label: "Claude", enabled: true, status: "ready", models: [] },
      {
        provider: "codex",
        label: "Codex",
        enabled: true,
        status: "ready",
        models: [],
        modes: [
          { id: "read-only", label: "Read only" },
          { id: "auto", label: "Auto" },
        ],
      },
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

function settings(view: BotView) {
  return (
    <QueryClientProvider client={queryClient}>
      <BotSettings
        serverId="host"
        workspaceId="workspace"
        cwd="/repo"
        bot={view}
        interactive
        onRemoved={onRemoved}
      />
    </QueryClientProvider>
  );
}

function renderSettings(view: BotView = bot) {
  return render(settings(view));
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
  supported.current = true;
  confirmDialog.mockClear();
  onRemoved.mockClear();
});
afterEach(() => {
  cleanup();
  queryClient.clear();
});

const withPermissions: BotView = {
  ...bot,
  permissions: {
    groups: ["observe", "agents", "stop", "schedules"],
    modes: { codex: ["read-only", "auto"] },
  },
};

describe("BotSettings permissions", () => {
  it("settings_saves_permissions_with_workspace_id", async () => {
    const saved: BotView = {
      ...withPermissions,
      permissions: {
        groups: ["observe", "agents", "stop"],
        modes: withPermissions.permissions!.modes,
      },
    };
    botRequest.mockImplementation(async (method: string) => {
      if (method === "bot.memory.get.request") return { text: "memo", revision: "r1" };
      if (method === "bot.permissions.update.request")
        return { bot: saved, appliesToNextTurn: true };
      throw new Error(`unexpected ${method}`);
    });
    const { rerender } = renderSettings(withPermissions);
    expect(screen.getByText("bots.modeCapWarning")).toBeTruthy();
    fireEvent.click(screen.getByTestId("bot-group-schedules"));
    fireEvent.click(screen.getByTestId("bot-permissions-save"));
    await waitFor(() =>
      expect(botRequest).toHaveBeenCalledWith("bot.permissions.update.request", {
        workspaceId: "workspace",
        permissions: saved.permissions,
      }),
    );
    await waitFor(() =>
      expect(queryClient.getQueryData(botKeys.bot("host", "workspace"))).toEqual(saved),
    );
    expect(await screen.findByText("bots.permissionsHint")).toBeTruthy();
    // The parent re-renders with the saved Bot from the cache; the hint stays visible.
    rerender(settings(saved));
    expect(screen.getByText("bots.permissionsHint")).toBeTruthy();
    expect(screen.getByTestId("bot-group-schedules").getAttribute("aria-checked")).toBe("false");
  });

  it("a_permission_change_from_elsewhere_replaces_the_draft", () => {
    const { rerender } = renderSettings(withPermissions);
    fireEvent.click(screen.getByTestId("bot-group-schedules"));
    expect(screen.getByTestId("bot-group-schedules").getAttribute("aria-checked")).toBe("false");
    const remote: BotView = {
      ...withPermissions,
      permissions: {
        groups: ["observe", "agents", "stop", "schedules", "terminals"],
        modes: { codex: ["auto"] },
      },
    };
    queryClient.setQueryData(botKeys.bot("host", "workspace"), remote);
    rerender(settings(remote));
    expect(screen.getByTestId("bot-group-schedules").getAttribute("aria-checked")).toBe("true");
    expect(screen.getByTestId("bot-group-terminals").getAttribute("aria-checked")).toBe("true");
    expect((screen.getByTestId("bot-mode-codex") as HTMLSelectElement).value).toBe("auto");
  });

  it("stays editable while the Bot is paused", () => {
    renderSettings({ ...withPermissions, state: { kind: "paused", reason: "user" } });
    expect(screen.getByTestId("bot-group-agents").getAttribute("aria-disabled")).not.toBe("true");
    expect(screen.getByTestId("bot-permissions-save")).toHaveProperty("disabled", false);
  });

  it("guest_sees_permissions_read_only", () => {
    canManage.mockReturnValue(false);
    renderSettings(withPermissions);
    expect(screen.getByTestId("bot-group-agents").getAttribute("aria-disabled")).toBe("true");
    expect(screen.getByTestId("bot-mode-codex")).toHaveProperty("disabled", true);
    expect(screen.queryByTestId("bot-permissions-save")).toBeNull();
  });

  it("hides permissions on hosts without the permissions method", () => {
    supported.current = false;
    renderSettings(withPermissions);
    expect(screen.queryByTestId("bot-permissions-section")).toBeNull();
    expect(screen.getByTestId("bot-provider")).toBeTruthy();
    expect(screen.getByTestId("bot-mode")).toBeTruthy();
    expect(screen.queryByText("bots.readOnlyHint")).toBeNull();
  });

  it("hides the mode picker on hosts with the permissions method", () => {
    renderSettings(withPermissions);
    expect(screen.queryByTestId("bot-mode")).toBeNull();
    expect(screen.getByText("bots.readOnlyHint")).toBeTruthy();
  });
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

  it("hides pause, remove and save actions and disables edits for read-only users", async () => {
    canManage.mockReturnValue(false);
    renderSettings({ ...bot, state: { kind: "paused", reason: "user" } });
    expect(screen.queryByText("bots.pause")).toBeNull();
    expect(screen.queryByText("bots.resume")).toBeNull();
    expect(screen.queryByText("bots.remove")).toBeNull();
    expect((screen.getByTestId("bot-provider") as HTMLSelectElement).disabled).toBe(true);
    expect(screen.queryByTestId("bot-settings-save")).toBeNull();
    expect((await screen.findByTestId("bot-memory")) as HTMLTextAreaElement).toHaveProperty(
      "disabled",
      true,
    );
    expect(screen.queryByTestId("bot-memory-save")).toBeNull();
    expect(screen.queryByText("bots.resumeToEdit")).toBeNull();
  });

  it("disables memory saves until a paused Bot is resumed", async () => {
    renderSettings({ ...bot, state: { kind: "paused", reason: "user" } });
    fireEvent.change(await screen.findByTestId("bot-memory"), { target: { value: "new" } });
    expect(screen.getByTestId("bot-memory-save")).toHaveProperty("disabled", true);
    expect(screen.getAllByText("bots.resumeToEdit").length).toBeGreaterThan(0);
    expect(screen.getByTestId("bot-settings-save")).toHaveProperty("disabled", false);
  });

  it("explains a save refused because the Bot was paused elsewhere", async () => {
    const paused: BotView = { ...bot, state: { kind: "paused", reason: "user" } };
    botRequest.mockImplementation(async (method: string) => {
      if (method === "bot.memory.get.request") return { text: "memo", revision: "r1" };
      if (method === "bot.memory.update.request")
        throw Object.assign(new Error("Bot conflict"), { code: "bot_conflict" });
      if (method === "bot.get.request") return { bot: paused };
      throw new Error(`unexpected ${method}`);
    });
    renderSettings();
    fireEvent.change(await screen.findByTestId("bot-memory"), { target: { value: "new" } });
    fireEvent.click(screen.getByTestId("bot-memory-save"));
    expect(await screen.findByText("bots.resumeToEdit")).toBeTruthy();
    expect(screen.queryByText("bots.memoryConflict")).toBeNull();
    expect(queryClient.getQueryData(botKeys.bot("host", "workspace"))).toEqual(paused);
  });

  it("reports a memory revision conflict while the Bot is active", async () => {
    botRequest.mockImplementation(async (method: string) => {
      if (method === "bot.memory.get.request") return { text: "memo", revision: "r1" };
      if (method === "bot.memory.update.request")
        throw Object.assign(new Error("Bot conflict"), { code: "bot_conflict" });
      if (method === "bot.get.request") return { bot };
      throw new Error(`unexpected ${method}`);
    });
    renderSettings();
    fireEvent.change(await screen.findByTestId("bot-memory"), { target: { value: "new" } });
    fireEvent.click(screen.getByTestId("bot-memory-save"));
    expect(await screen.findByText("bots.memoryConflict")).toBeTruthy();
    expect(screen.queryByText("bots.resumeToEdit")).toBeNull();
  });
});

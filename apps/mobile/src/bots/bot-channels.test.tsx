// @vitest-environment jsdom
import React from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BotChannels } from "./bot-channels";

const { botRequest, confirmDialog, randomUUID } = vi.hoisted(() => ({
  botRequest: vi.fn(),
  confirmDialog: vi.fn(async (_options: unknown) => true),
  randomUUID: vi.fn(() => "33333333-3333-4333-8333-333333333333"),
}));
vi.mock("@/runtime/host-runtime", () => ({ useHostRuntimeClient: () => ({ botRequest }) }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("./styles", () => ({ botStyles: {} }));
vi.mock("./loading", () => ({ BotLoadingSpinner: () => null }));
vi.mock("@/utils/confirm-dialog", () => ({ confirmDialog }));
vi.mock("expo-crypto", () => ({ randomUUID }));
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
    <select data-testid={testID} value={value} onChange={(e) => onChange(e.target.value)}>
      {options.map((option) => (
        <option key={option.id} value={option.id}>
          {option.label}
        </option>
      ))}
    </select>
  ),
}));
vi.mock("@/components/ui/switch", () => ({
  Switch: ({
    value,
    onValueChange,
    disabled,
    accessibilityLabel,
    testID,
  }: {
    value: boolean;
    onValueChange: (value: boolean) => void;
    disabled?: boolean;
    accessibilityLabel?: string;
    testID?: string;
  }) => (
    <input
      type="checkbox"
      data-testid={testID}
      aria-label={accessibilityLabel}
      checked={value}
      disabled={disabled}
      onChange={() => onValueChange(!value)}
    />
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
  Field: ({ label, children }: { label: string; children: React.ReactNode }) => (
    <div>
      <span>{label}</span>
      {children}
    </div>
  ),
  FormTextInput: ({
    onChangeText,
    editable,
    accessibilityLabel,
  }: {
    onChangeText: (text: string) => void;
    editable?: boolean;
    accessibilityLabel?: string;
  }) => (
    <input
      aria-label={accessibilityLabel}
      disabled={editable === false}
      onChange={(event) => onChangeText(event.target.value)}
    />
  ),
}));

const ID_A = "11111111-1111-4111-8111-111111111111";
const ID_B = "22222222-2222-4222-8222-222222222222";
const ID_C = "44444444-4444-4444-8444-444444444444";
const ID_D = "55555555-5555-4555-8555-555555555555";
const telegram = {
  kind: "telegram",
  tokenRef: "env:AIT_SERVER_CREDENTIAL_A",
  allowedSenders: ["1"],
  allowGroups: false,
};
const channels = [
  { id: ID_A, config: { kind: "builtin" }, enabled: true, cursor: null, deleted: false },
  { id: ID_B, config: telegram, enabled: false, cursor: null, deleted: false },
  {
    id: ID_C,
    config: {
      kind: "feishu",
      appId: "cli_x",
      secretRef: "env:AIT_SERVER_CREDENTIAL_F",
      allowedSenders: ["u"],
      allowGroups: true,
    },
    enabled: true,
    cursor: null,
    deleted: false,
  },
  { id: ID_D, config: telegram, enabled: false, cursor: null, deleted: true },
];
let queryClient: QueryClient;

function renderChannels({ readOnly = false, active = true } = {}) {
  return render(
    <QueryClientProvider client={queryClient}>
      <BotChannels
        serverId="host"
        workspaceId="workspace"
        builtinChannelId={ID_A}
        interactive
        readOnly={readOnly}
        active={active}
      />
    </QueryClientProvider>,
  );
}
function typeInto(label: string, value: string) {
  fireEvent.change(screen.getByLabelText(label), { target: { value } });
}
function submitButton() {
  return screen.getByTestId("bot-channel-submit");
}

beforeEach(() => {
  queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  botRequest.mockReset();
  botRequest.mockImplementation(async (method: string, params: Record<string, unknown>) => {
    if (method === "bot.channel.list.request") return { channels };
    if (method === "bot.channel.types.request")
      return {
        supported: ["builtin", "telegram", "feishu"],
        email: { supported: false, planned: true },
      };
    if (method === "bot.channel.status.request")
      return {
        channelId: params.channelId,
        enabled: true,
        connection: params.channelId === ID_C ? { state: "connecting" } : {},
      };
    if (method === "bot.channel.create.request")
      return { channel: { ...channels[1], id: params.channelId }, created: true };
    if (method === "bot.channel.update.request")
      return { channel: { ...channels[1], enabled: params.enabled } };
    if (method === "bot.channel.delete.request")
      return { deleted: true, channelId: params.channelId };
    throw new Error(`unexpected ${method}`);
  });
  confirmDialog.mockClear();
});
afterEach(() => {
  cleanup();
  queryClient.clear();
});

describe("BotChannels", () => {
  it("channel_form_requires_env_reference_and_senders", async () => {
    renderChannels();
    fireEvent.click(await screen.findByText("bots.addChannel"));
    await screen.findByLabelText("bots.tokenRef");
    typeInto("bots.tokenRef", "123456:plain-token");
    typeInto("bots.allowedSenders", "42");
    expect(submitButton()).toHaveProperty("disabled", true);
    expect(screen.getByText("bots.channelRefHint")).toBeTruthy();
    typeInto("bots.tokenRef", "env:AIT_SERVER_CREDENTIAL_TELEGRAM");
    expect(submitButton()).toHaveProperty("disabled", false);
    typeInto("bots.allowedSenders", "");
    expect(submitButton()).toHaveProperty("disabled", true);
  });

  it("creates a disabled telegram channel with workspaceId", async () => {
    renderChannels();
    fireEvent.click(await screen.findByText("bots.addChannel"));
    await screen.findByLabelText("bots.tokenRef");
    typeInto("bots.tokenRef", "env:AIT_SERVER_CREDENTIAL_TELEGRAM");
    typeInto("bots.allowedSenders", "42, 43");
    fireEvent.click(submitButton());
    await waitFor(() =>
      expect(botRequest).toHaveBeenCalledWith("bot.channel.create.request", {
        workspaceId: "workspace",
        channelId: "33333333-3333-4333-8333-333333333333",
        enabled: false,
        config: {
          kind: "telegram",
          tokenRef: "env:AIT_SERVER_CREDENTIAL_TELEGRAM",
          allowedSenders: ["42", "43"],
          allowGroups: false,
        },
      }),
    );
  });

  it("creates a feishu channel from its own fields", async () => {
    renderChannels();
    fireEvent.click(await screen.findByText("bots.addChannel"));
    await screen.findByLabelText("bots.tokenRef");
    fireEvent.change(screen.getByTestId("bot-channel-type"), { target: { value: "feishu" } });
    typeInto("bots.appId", "cli_app");
    typeInto("bots.secretRef", "env:AIT_SERVER_CREDENTIAL_FEISHU");
    typeInto("bots.allowedSenders", "a, b");
    fireEvent.click(submitButton());
    await waitFor(() =>
      expect(botRequest).toHaveBeenCalledWith(
        "bot.channel.create.request",
        expect.objectContaining({
          config: {
            kind: "feishu",
            appId: "cli_app",
            secretRef: "env:AIT_SERVER_CREDENTIAL_FEISHU",
            allowedSenders: ["a", "b"],
            allowGroups: false,
          },
        }),
      ),
    );
  });

  it("shows server errors on create", async () => {
    botRequest.mockImplementation(async (method: string) => {
      if (method === "bot.channel.list.request") return { channels: [] };
      if (method === "bot.channel.types.request")
        return { supported: ["telegram"], email: { supported: false, planned: false } };
      throw new Error("account already in use");
    });
    renderChannels();
    fireEvent.click(await screen.findByText("bots.addChannel"));
    await screen.findByLabelText("bots.tokenRef");
    typeInto("bots.tokenRef", "env:AIT_SERVER_CREDENTIAL_T");
    typeInto("bots.allowedSenders", "1");
    fireEvent.click(submitButton());
    expect(await screen.findByText("account already in use")).toBeTruthy();
  });

  it("keeps the channel id across retries of one form and renews it for the next form", async () => {
    randomUUID
      .mockReturnValueOnce("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa")
      .mockReturnValueOnce("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb");
    let creates = 0;
    const fallback = botRequest.getMockImplementation()!;
    botRequest.mockImplementation(async (method: string, params: Record<string, unknown>) => {
      if (method === "bot.channel.create.request" && ++creates < 3)
        throw new Error("Response lost");
      return fallback(method, params);
    });
    renderChannels();
    fireEvent.click(await screen.findByText("bots.addChannel"));
    await screen.findByLabelText("bots.tokenRef");
    typeInto("bots.tokenRef", "env:AIT_SERVER_CREDENTIAL_TELEGRAM");
    typeInto("bots.allowedSenders", "42");
    fireEvent.click(submitButton());
    await screen.findByText("Response lost");
    fireEvent.click(submitButton());
    await waitFor(() => expect(creates).toBe(2));
    await waitFor(() => expect(submitButton()).toHaveProperty("disabled", false));
    fireEvent.click(submitButton());
    await waitFor(() => expect(screen.queryByTestId("bot-channel-form")).toBeNull());
    const ids = () =>
      botRequest.mock.calls
        .filter(([method]) => method === "bot.channel.create.request")
        .map(([, params]) => params.channelId);
    expect(ids()).toEqual([
      "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
      "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
      "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    ]);

    fireEvent.click(screen.getByText("bots.addChannel"));
    await screen.findByLabelText("bots.tokenRef");
    typeInto("bots.tokenRef", "env:AIT_SERVER_CREDENTIAL_TELEGRAM");
    typeInto("bots.allowedSenders", "43");
    fireEvent.click(submitButton());
    await waitFor(() => expect(ids()).toHaveLength(4));
    expect(ids()[3]).toBe("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb");
  });

  it("disables adding and toggling while the Bot is paused", async () => {
    renderChannels({ active: false });
    await screen.findByTestId(`bot-channel-${ID_B}`);
    expect(await screen.findByText("bots.resumeToEdit")).toBeTruthy();
    expect(screen.getByTestId("bot-channel-add")).toHaveProperty("disabled", true);
    expect(screen.getByTestId(`bot-channel-enabled-${ID_B}`)).toHaveProperty("disabled", true);
    expect(screen.getByTestId(`bot-channel-delete-${ID_B}`)).toHaveProperty("disabled", false);
  });

  it("hides the resume hint from guests and while the Bot is active", async () => {
    renderChannels();
    await screen.findByTestId(`bot-channel-${ID_B}`);
    expect(screen.queryByText("bots.resumeToEdit")).toBeNull();
    cleanup();
    renderChannels({ readOnly: true, active: false });
    await screen.findByTestId(`bot-channel-${ID_B}`);
    expect(screen.queryByText("bots.resumeToEdit")).toBeNull();
  });

  it("lists only external channels and toggles enabled", async () => {
    renderChannels();
    expect(await screen.findByTestId(`bot-channel-${ID_B}`)).toBeTruthy();
    expect(screen.getByTestId(`bot-channel-${ID_C}`)).toBeTruthy();
    expect(screen.queryByTestId(`bot-channel-${ID_A}`)).toBeNull();
    expect(screen.queryByTestId(`bot-channel-${ID_D}`)).toBeNull();
    expect(await screen.findByText("connecting")).toBeTruthy();
    fireEvent.click(screen.getByTestId(`bot-channel-enabled-${ID_B}`));
    await waitFor(() =>
      expect(botRequest).toHaveBeenCalledWith("bot.channel.update.request", {
        workspaceId: "workspace",
        channelId: ID_B,
        enabled: true,
      }),
    );
  });

  it("deletes after confirmation", async () => {
    renderChannels();
    fireEvent.click(await screen.findByTestId(`bot-channel-delete-${ID_B}`));
    await waitFor(() =>
      expect(botRequest).toHaveBeenCalledWith("bot.channel.delete.request", {
        workspaceId: "workspace",
        channelId: ID_B,
      }),
    );
    expect(confirmDialog).toHaveBeenCalledOnce();
  });

  it("is read-only for guests", async () => {
    renderChannels({ readOnly: true });
    await screen.findByTestId(`bot-channel-${ID_B}`);
    expect(screen.queryByText("bots.addChannel")).toBeNull();
    expect(screen.queryByTestId(`bot-channel-delete-${ID_B}`)).toBeNull();
    expect(screen.getByTestId(`bot-channel-enabled-${ID_B}`)).toHaveProperty("disabled", true);
    await act(async () => {});
  });
});

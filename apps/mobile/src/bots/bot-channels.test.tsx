// @vitest-environment jsdom
import React from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BotChannels } from "./bot-channels";

const { botRequest, confirmDialog, randomUUID, feishuSetup } = vi.hoisted(() => ({
  feishuSetup: { current: false },
  botRequest: vi.fn(),
  confirmDialog: vi.fn(async (_options: unknown) => true),
  randomUUID: vi.fn(() => "33333333-3333-4333-8333-333333333333"),
}));
vi.mock("@/runtime/host-runtime", () => ({ useHostRuntimeClient: () => ({ botRequest }) }));
vi.mock("@/stores/session-store", () => ({
  useSessionStore: (select: (state: unknown) => unknown) =>
    select({
      sessions: {
        host: { serverInfo: { features: { workspaceBotFeishuSetup: feishuSetup.current } } },
      },
    }),
}));
vi.mock("react-native-svg", () => ({
  SvgXml: ({ xml }: { xml: string }) => <span data-testid="qr-xml">{xml}</span>,
}));
vi.mock("@/utils/open-external-url", () => ({ openExternalUrl: vi.fn() }));
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
  feishuSetup.current = false;
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

  describe("Feishu setup", () => {
    const SCAN_ID = "66666666-6666-4666-8666-666666666666";
    const URL_TO_SCAN = "https://open.feishu.cn/page/launcher?user_code=A";
    function scanResponses(status: Record<string, unknown>) {
      const base = botRequest.getMockImplementation();
      botRequest.mockImplementation(async (method: string, params: Record<string, unknown>) => {
        if (method === "bot.channel.feishu.scan.start.request") {
          return { scanId: SCAN_ID, url: URL_TO_SCAN, expiresInSeconds: 3600 };
        }
        if (method === "bot.channel.feishu.scan.status.request") return status;
        if (method === "bot.channel.feishu.scan.cancel.request") return { cancelled: true };
        if (method === "bot.channel.feishu.connect.request") {
          return { channel: channels[2], created: true };
        }
        return base?.(method, params);
      });
    }
    async function openForm() {
      feishuSetup.current = true;
      renderChannels();
      fireEvent.click(await screen.findByTestId("bot-channel-add"));
      await screen.findByTestId("bot-channel-form");
    }

    it("defaults to scanning and shows a QR code while waiting", async () => {
      scanResponses({ state: "waiting" });
      await openForm();
      fireEvent.click(screen.getByTestId("bot-channel-allow-groups"));
      fireEvent.click(screen.getByTestId("bot-feishu-scan-start"));

      expect((await screen.findByTestId("qr-xml")).textContent).toContain("<svg");
      expect(botRequest).toHaveBeenCalledWith("bot.channel.feishu.scan.start.request", {
        workspaceId: "workspace",
        allowGroups: true,
      });
      await waitFor(() =>
        expect(botRequest).toHaveBeenCalledWith("bot.channel.feishu.scan.status.request", {
          workspaceId: "workspace",
          scanId: SCAN_ID,
        }),
      );
      expect(screen.queryByLabelText("bots.appId")).toBeNull();
    });

    it("closes the form and refreshes when the scan completes", async () => {
      scanResponses({ state: "complete", channel: channels[2] });
      await openForm();
      fireEvent.click(screen.getByTestId("bot-feishu-scan-start"));

      await waitFor(() => expect(screen.queryByTestId("bot-channel-form")).toBeNull());
    });

    it("explains a failed scan and can try again", async () => {
      scanResponses({ state: "failed", reason: "denied" });
      await openForm();
      fireEvent.click(screen.getByTestId("bot-feishu-scan-start"));

      expect((await screen.findByTestId("bot-feishu-scan-failed")).textContent).toBe(
        "bots.feishuScanDenied",
      );
      expect(screen.getByTestId("bot-feishu-scan-retry")).toBeTruthy();
      expect(screen.queryByTestId("qr-xml")).toBeNull();
    });

    it("cancels a waiting scan on the host", async () => {
      scanResponses({ state: "waiting" });
      await openForm();
      fireEvent.click(screen.getByTestId("bot-feishu-scan-start"));
      fireEvent.click(await screen.findByTestId("bot-feishu-scan-cancel"));

      await waitFor(() =>
        expect(botRequest).toHaveBeenCalledWith("bot.channel.feishu.scan.cancel.request", {
          workspaceId: "workspace",
          scanId: SCAN_ID,
        }),
      );
      expect(await screen.findByTestId("bot-feishu-scan-start")).toBeTruthy();
    });

    it("cancels a waiting scan when the form switches away from Feishu", async () => {
      scanResponses({ state: "waiting" });
      await openForm();
      fireEvent.click(screen.getByTestId("bot-feishu-scan-start"));
      await screen.findByTestId("qr-xml");
      fireEvent.change(screen.getByTestId("bot-channel-type"), { target: { value: "telegram" } });

      await waitFor(() =>
        expect(botRequest).toHaveBeenCalledWith("bot.channel.feishu.scan.cancel.request", {
          workspaceId: "workspace",
          scanId: SCAN_ID,
        }),
      );
    });

    it("sends a pasted App Secret through the connect request", async () => {
      scanResponses({ state: "waiting" });
      await openForm();
      fireEvent.click(screen.getByTestId("bot-feishu-manual"));
      expect(await screen.findByTestId("bot-feishu-manual-guide")).toBeTruthy();
      typeInto("bots.appId", "cli_x");
      typeInto("bots.feishuAppSecret", " pasted-secret\n");
      typeInto("bots.allowedSenders", "ou_a");
      fireEvent.click(submitButton());

      await waitFor(() =>
        expect(botRequest).toHaveBeenCalledWith("bot.channel.feishu.connect.request", {
          workspaceId: "workspace",
          channelId: "33333333-3333-4333-8333-333333333333",
          appId: "cli_x",
          appSecret: "pasted-secret",
          allowedSenders: ["ou_a"],
          allowGroups: false,
        }),
      );
    });

    it("keeps environment references on the original create request", async () => {
      scanResponses({ state: "waiting" });
      await openForm();
      fireEvent.click(screen.getByTestId("bot-feishu-manual"));
      typeInto("bots.appId", "cli_x");
      typeInto("bots.feishuAppSecret", "env:AIT_SERVER_CREDENTIAL_F");
      typeInto("bots.allowedSenders", "ou_a");
      fireEvent.click(submitButton());

      await waitFor(() =>
        expect(botRequest).toHaveBeenCalledWith(
          "bot.channel.create.request",
          expect.objectContaining({
            enabled: false,
            config: expect.objectContaining({ secretRef: "env:AIT_SERVER_CREDENTIAL_F" }),
          }),
        ),
      );
    });

    it("hides scanning from hosts that cannot run it", async () => {
      renderChannels();
      fireEvent.click(await screen.findByTestId("bot-channel-add"));
      fireEvent.change(await screen.findByTestId("bot-channel-type"), {
        target: { value: "feishu" },
      });

      expect(await screen.findByLabelText("bots.secretRef")).toBeTruthy();
      expect(screen.queryByTestId("bot-feishu-scan-start")).toBeNull();
    });
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

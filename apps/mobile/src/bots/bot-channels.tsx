import React, { useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { randomUUID } from "expo-crypto";
import type { z } from "zod";
import type { BotChannelSchema } from "@ait/protocol/bot/types";
import { SettingsCard, SettingsGroup, SettingsRow, SettingsSection } from "@/components/settings";
import { Button } from "@/components/ui/button";
import { Field, FormTextInput } from "@/components/ui/form-field";
import { SelectField } from "@/components/ui/select-field";
import { Switch } from "@/components/ui/switch";
import { useHostFeature } from "@/runtime/host-features";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { confirmDialog } from "@/utils/confirm-dialog";
import { BotChatPolicyRow, useBotChats } from "./bot-chat-policies";
import { AllowGroupsCheckbox } from "./allow-groups-checkbox";
import { FeishuManualGuide, FeishuScanSetup } from "./feishu-setup";
import { TextLink } from "./channel-setup";
import { ChannelScanSetup, QqManualSetup } from "./scan-setup";
import { ImessageGuidedSetup } from "./imessage-setup";
import { BotLoadingSpinner } from "./loading";
import { botKeys } from "./queries";
import { KIND_LABELS } from "./channel-kinds";
import { botStyles as styles } from "./styles";
import { TelegramGuidedSetup } from "./telegram-setup";
import { WebhookDetails, WebhookSetup } from "./webhook-setup";

type BotChannel = z.infer<typeof BotChannelSchema>;

const CREDENTIAL_REF = /^env:AIT_SERVER_CREDENTIAL_[A-Z0-9_]{1,96}$/;
type ExternalKind =
  | "telegram"
  | "feishu"
  | "lark"
  | "weixin"
  | "qq"
  | "photon_imessage"
  | "webhook";
function connectionSummary(connection: unknown, fallback: string): string {
  if (connection && typeof connection === "object") {
    const { error, state } = connection as { error?: unknown; state?: unknown };
    if (typeof error === "string" && error) return error;
    if (typeof state === "string" && state) return state;
  }
  return fallback;
}

function ChannelRow({
  serverId,
  workspaceId,
  builtinChannelId,
  channel,
  interactive,
  readOnly,
  active,
  onChanged,
  chatList,
}: {
  serverId: string;
  workspaceId: string;
  builtinChannelId: string;
  channel: BotChannel;
  interactive: boolean;
  readOnly: boolean;
  active: boolean;
  onChanged: () => void;
  chatList?: ReturnType<typeof useBotChats>;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const status = useQuery({
    queryKey: [
      ...botKeys.channels(serverId, workspaceId, builtinChannelId),
      "status",
      channel.id,
      channel.enabled,
    ],
    enabled: interactive && Boolean(client),
    refetchInterval: 15_000,
    queryFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      return (
        await client.botRequest("bot.channel.status.request", {
          workspaceId,
          channelId: channel.id,
        })
      ).connection;
    },
  });
  const toggle = useMutation({
    mutationFn: async (enabled: boolean) => {
      if (!client) throw new Error(t("bots.disconnected"));
      await client.botRequest("bot.channel.update.request", {
        workspaceId,
        channelId: channel.id,
        enabled,
      });
    },
    onSuccess: onChanged,
  });
  const remove = useMutation({
    mutationFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      if (
        !(await confirmDialog({
          title: t("bots.deleteChannel"),
          message: t("bots.deleteChannelConfirm"),
          confirmLabel: t("bots.deleteChannel"),
          cancelLabel: t("common.actions.cancel"),
          destructive: true,
        }))
      )
        return;
      await client.botRequest("bot.channel.delete.request", {
        workspaceId,
        channelId: channel.id,
      });
      onChanged();
    },
  });
  const busy = toggle.isPending || remove.isPending;
  const kind = channel.config.kind === "builtin" ? "" : t(KIND_LABELS[channel.config.kind]);
  const connection = status.data
    ? connectionSummary(
        status.data,
        channel.enabled ? t("bots.channelConnected") : t("bots.channelIdle"),
      )
    : null;
  const config = channel.config;
  const account = "appId" in config ? `${kind} · ${config.appId}` : kind;
  const chats = chatList?.data?.filter((chat) => chat.channelId === channel.id);
  return (
    <SettingsSection
      title={account}
      info={chatList ? t("bots.chatReceptionHint") : undefined}
      testID={`bot-channel-${channel.id}`}
    >
      <SettingsCard testID={`bot-channel-${channel.id}-card`}>
        <SettingsRow
          label={t("bots.channelEnabled")}
          hint={connection ?? undefined}
          error={toggle.error?.message ?? remove.error?.message}
        >
          <View style={styles.row}>
            <Switch
              value={channel.enabled}
              onValueChange={(enabled) => toggle.mutate(enabled)}
              disabled={readOnly || !active || !interactive || busy}
              accessibilityLabel={t("bots.channelEnabled")}
              testID={`bot-channel-enabled-${channel.id}`}
            />
            {!readOnly ? (
              <Button
                variant="outline"
                size="sm"
                disabled={!interactive || busy}
                onPress={() => remove.mutate()}
                testID={`bot-channel-delete-${channel.id}`}
              >
                {t("bots.deleteChannel")}
              </Button>
            ) : null}
          </View>
        </SettingsRow>
        {channel.config.kind === "webhook" && !readOnly ? (
          <View style={styles.cardBlock} testID={`bot-webhook-details-${channel.id}`}>
            <WebhookDetails
              serverId={serverId}
              workspaceId={workspaceId}
              channelId={channel.id}
              url={channel.config.url}
              disabled={!interactive || busy}
            />
          </View>
        ) : null}
        {chatList?.isPending && interactive ? (
          <SettingsRow label={t("bots.chatReception")}>
            <BotLoadingSpinner />
          </SettingsRow>
        ) : null}
        {chatList?.error ? (
          <SettingsRow label={t("bots.chatReception")} error={chatList.error.message}>
            <Button
              variant="outline"
              size="sm"
              disabled={!interactive || chatList.isFetching}
              onPress={() => {
                void chatList.refetch();
              }}
              testID={`bot-chat-retry-${channel.id}`}
            >
              {t("bots.reload")}
            </Button>
          </SettingsRow>
        ) : null}
        {chats?.length === 0 ? (
          <SettingsRow label={t("bots.chatReception")} hint={t("bots.noExternalChats")} />
        ) : null}
        {chats?.map((chat) => (
          <BotChatPolicyRow
            key={chat.peer}
            serverId={serverId}
            workspaceId={workspaceId}
            builtinChannelId={builtinChannelId}
            chat={chat}
            platform={kind}
            disabled={readOnly || !active || !interactive}
          />
        ))}
      </SettingsCard>
    </SettingsSection>
  );
}

function ChannelForm({
  serverId,
  workspaceId,
  kinds,
  active,
  onDone,
}: {
  serverId: string;
  workspaceId: string;
  kinds: ExternalKind[];
  active: boolean;
  onDone: () => void;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  // One id per opened form: a retry after a lost response must not create a second channel.
  const [channelId] = useState(() => randomUUID());
  const perChatReception = useHostFeature(serverId, "workspaceBotChatReception");
  const feishuSetupSupported = useHostFeature(serverId, "workspaceBotFeishuSetup");
  const larkSetupSupported = useHostFeature(serverId, "workspaceBotLarkSetup");
  const [kind, setKind] = useState<ExternalKind>(
    feishuSetupSupported && kinds.includes("feishu") ? "feishu" : (kinds[0] ?? "telegram"),
  );
  const [tokenRef, setTokenRef] = useState("");
  const [appId, setAppId] = useState("");
  const [secretRef, setSecretRef] = useState("");
  const [allowGroups, setAllowGroups] = useState(false);
  const [enterManually, setEnterManually] = useState(false);
  const scanning = kind === "feishu" && feishuSetupSupported && !enterManually;
  const telegramSetupSupported = useHostFeature(serverId, "workspaceBotTelegramSetup");
  const [useEnvironment, setUseEnvironment] = useState(false);
  const [enterQqManually, setEnterQqManually] = useState(false);
  const telegramGuided = kind === "telegram" && telegramSetupSupported && !useEnvironment;
  const appSetupSupported = kind === "lark" ? larkSetupSupported : feishuSetupSupported;
  // A pasted App Secret is verified in its own region; a reference still uses the variable.
  const pastedSecret =
    (kind === "feishu" || kind === "lark") && appSetupSupported && !CREDENTIAL_REF.test(secretRef);
  const valid =
    kind === "telegram"
      ? CREDENTIAL_REF.test(tokenRef)
      : appId.trim().length > 0 &&
        (CREDENTIAL_REF.test(secretRef) || (pastedSecret && secretRef.trim().length > 0));
  const create = useMutation({
    mutationFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      // These render their own setup and never reach this form's submit button.
      if (kind === "weixin" || kind === "qq" || kind === "photon_imessage" || kind === "webhook")
        return;
      if (pastedSecret) {
        await client.botRequest(
          kind === "lark"
            ? "bot.channel.lark.connect.request"
            : "bot.channel.feishu.connect.request",
          {
            workspaceId,
            channelId,
            appId: appId.trim(),
            appSecret: secretRef.trim(),
            allowedSenders: [],
            allowGroups,
          },
        );
        return;
      }
      await client.botRequest("bot.channel.create.request", {
        workspaceId,
        channelId,
        enabled: false,
        config:
          kind === "telegram"
            ? { kind, tokenRef, allowedSenders: [], allowGroups }
            : { kind, appId: appId.trim(), secretRef, allowedSenders: [], allowGroups },
      });
    },
    onSuccess: onDone,
  });
  const showHint = !pastedSecret && (kind === "telegram" ? tokenRef : secretRef) !== "" && !valid;
  const typeField = (
    <SelectField
      label={t("bots.channelType")}
      value={kind}
      selectedDisplay={{ label: t(KIND_LABELS[kind]) }}
      options={kinds.map((id) => ({
        id,
        value: id,
        label: t(KIND_LABELS[id]),
      }))}
      onChange={(value) => setKind(value as ExternalKind)}
      disabled={create.isPending}
      placeholder={t("bots.channelType")}
      emptyText={t("bots.channelType")}
      testID="bot-channel-type"
    />
  );
  const card = (body: React.ReactNode) => (
    <View style={styles.card} testID="bot-channel-form">
      {typeField}
      {body}
    </View>
  );
  if (kind === "weixin" || (kind === "qq" && !enterQqManually)) {
    return card(
      <ChannelScanSetup
        key={kind}
        kind={kind}
        serverId={serverId}
        workspaceId={workspaceId}
        active={active}
        onDone={onDone}
        onEnterManually={kind === "qq" ? () => setEnterQqManually(true) : undefined}
      />,
    );
  }
  if (kind === "qq") {
    return card(
      <QqManualSetup
        serverId={serverId}
        workspaceId={workspaceId}
        channelId={channelId}
        active={active}
        onDone={onDone}
        onScanInstead={() => setEnterQqManually(false)}
      />,
    );
  }
  if (kind === "webhook") {
    return card(
      <WebhookSetup
        serverId={serverId}
        workspaceId={workspaceId}
        active={active}
        onDone={onDone}
      />,
    );
  }
  if (kind === "photon_imessage") {
    return card(
      <ImessageGuidedSetup
        serverId={serverId}
        workspaceId={workspaceId}
        active={active}
        onDone={onDone}
      />,
    );
  }
  if (scanning) {
    return card(
      <FeishuScanSetup
        serverId={serverId}
        workspaceId={workspaceId}
        active={active}
        onDone={onDone}
        onEnterManually={() => setEnterManually(true)}
      />,
    );
  }
  if (telegramGuided) {
    return card(
      <TelegramGuidedSetup
        serverId={serverId}
        workspaceId={workspaceId}
        active={active}
        onDone={onDone}
        onUseEnvironment={() => setUseEnvironment(true)}
      />,
    );
  }
  return (
    <View style={styles.card} testID="bot-channel-form">
      {typeField}
      {kind === "telegram" && telegramSetupSupported ? (
        <TextLink
          label={t("bots.telegramUseGuided")}
          onPress={() => setUseEnvironment(false)}
          testID="bot-telegram-use-guided"
        />
      ) : null}
      {(kind === "feishu" || kind === "lark") && appSetupSupported ? (
        <FeishuManualGuide
          platform={kind}
          onScanInstead={kind === "feishu" ? () => setEnterManually(false) : undefined}
        />
      ) : null}
      {kind === "telegram" ? (
        <Field label={t("bots.tokenRef")} hint={t("bots.channelRefHint")}>
          <FormTextInput
            initialValue={tokenRef}
            onChangeText={setTokenRef}
            editable={!create.isPending}
            accessibilityLabel={t("bots.tokenRef")}
            autoCapitalize="none"
            autoCorrect={false}
          />
        </Field>
      ) : (
        <>
          <Field label={t("bots.appId")}>
            <FormTextInput
              initialValue={appId}
              onChangeText={setAppId}
              editable={!create.isPending}
              accessibilityLabel={t("bots.appId")}
              autoCapitalize="none"
              autoCorrect={false}
            />
          </Field>
          <Field
            label={appSetupSupported ? t("bots.feishuAppSecret") : t("bots.secretRef")}
            hint={
              appSetupSupported
                ? t("bots.feishuAppSecretHint", { platform: t(KIND_LABELS[kind]) })
                : t("bots.channelRefHint")
            }
          >
            <FormTextInput
              initialValue={secretRef}
              onChangeText={setSecretRef}
              editable={!create.isPending}
              secureTextEntry={appSetupSupported}
              accessibilityLabel={
                appSetupSupported ? t("bots.feishuAppSecret") : t("bots.secretRef")
              }
              autoCapitalize="none"
              autoCorrect={false}
            />
          </Field>
        </>
      )}
      {showHint ? <Text style={styles.error}>{t("bots.channelRefHint")}</Text> : null}
      {!(perChatReception && (kind === "feishu" || kind === "lark")) ? (
        <AllowGroupsCheckbox
          checked={allowGroups}
          disabled={create.isPending}
          onToggle={() => setAllowGroups((checked) => !checked)}
        />
      ) : null}
      {create.error ? (
        <Text style={styles.error} accessibilityRole="alert">
          {create.error.message}
        </Text>
      ) : null}
      <View style={styles.row}>
        <Button
          onPress={() => create.mutate()}
          disabled={!valid || !active || create.isPending}
          testID="bot-channel-submit"
        >
          {t("bots.addChannel")}
        </Button>
        <Button variant="outline" onPress={onDone} disabled={create.isPending}>
          {t("common.actions.cancel")}
        </Button>
      </View>
    </View>
  );
}

export function BotChannels({
  serverId,
  workspaceId,
  builtinChannelId,
  interactive,
  readOnly,
  active,
}: {
  serverId: string;
  workspaceId: string;
  builtinChannelId: string;
  interactive: boolean;
  readOnly: boolean;
  /** The host accepts channel creation and toggles only while the Bot is active. */
  active: boolean;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const queryClient = useQueryClient();
  const [adding, setAdding] = useState(false);
  const weixinSupported = useHostFeature(serverId, "workspaceBotWeixinSetup");
  const larkSupported = useHostFeature(serverId, "workspaceBotLarkSetup");
  const qqSupported = useHostFeature(serverId, "workspaceBotQqSetup");
  const imessageSupported = useHostFeature(serverId, "workspaceBotImessageSetup");
  const webhookSupported = useHostFeature(serverId, "workspaceBotWebhookSetup");
  const receptionSupported = useHostFeature(serverId, "workspaceBotChatReception");
  const enabled = interactive && Boolean(client);
  const list = useQuery({
    queryKey: botKeys.channels(serverId, workspaceId, builtinChannelId),
    enabled,
    queryFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      return (await client.botRequest("bot.channel.list.request", { workspaceId })).channels;
    },
  });
  const types = useQuery({
    queryKey: [...botKeys.channels(serverId, workspaceId, builtinChannelId), "types"],
    enabled,
    queryFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      return (await client.botRequest("bot.channel.types.request", {})).supported;
    },
  });
  const refresh = () =>
    queryClient.invalidateQueries({
      queryKey: botKeys.channels(serverId, workspaceId, builtinChannelId),
    });
  const external = (list.data ?? []).filter(
    (channel) => !channel.deleted && channel.config.kind !== "builtin",
  );
  const hasChatChannels = external.some(
    (channel) => channel.config.kind === "feishu" || channel.config.kind === "lark",
  );
  const chats = useBotChats(
    serverId,
    workspaceId,
    builtinChannelId,
    receptionSupported && enabled && !readOnly && hasChatChannels,
  );
  // Older clients reject unknown values in `supported`, so hosts advertise these by capability.
  const kinds: ExternalKind[] = types.data
    ? [
        ...types.data.filter(
          (kind): kind is "telegram" | "feishu" => kind === "telegram" || kind === "feishu",
        ),
        ...(larkSupported ? (["lark"] as const) : []),
        ...(weixinSupported ? (["weixin"] as const) : []),
        ...(qqSupported ? (["qq"] as const) : []),
        ...(imessageSupported ? (["photon_imessage"] as const) : []),
        ...(webhookSupported ? (["webhook"] as const) : []),
      ]
    : [];
  return (
    <SettingsGroup title={t("bots.channels")} testID="bot-channels">
      {list.isPending && enabled ? <BotLoadingSpinner /> : null}
      {list.error ? (
        <SettingsCard>
          <SettingsRow label={t("bots.channels")} error={list.error.message}>
            <Button
              size="sm"
              variant="outline"
              disabled={!enabled || list.isFetching}
              onPress={() => {
                void list.refetch();
              }}
            >
              {t("bots.reload")}
            </Button>
          </SettingsRow>
        </SettingsCard>
      ) : null}
      {!readOnly && !active ? <Text style={styles.hint}>{t("bots.resumeToEdit")}</Text> : null}
      {external.map((channel) => (
        <ChannelRow
          key={channel.id}
          serverId={serverId}
          workspaceId={workspaceId}
          builtinChannelId={builtinChannelId}
          channel={channel}
          interactive={interactive}
          readOnly={readOnly}
          active={active}
          onChanged={refresh}
          chatList={
            !readOnly &&
            receptionSupported &&
            (channel.config.kind === "feishu" || channel.config.kind === "lark")
              ? chats
              : undefined
          }
        />
      ))}
      {!readOnly && adding && kinds.length > 0 ? (
        <ChannelForm
          serverId={serverId}
          workspaceId={workspaceId}
          kinds={kinds}
          active={active}
          onDone={() => {
            setAdding(false);
            void refresh();
          }}
        />
      ) : null}
      {!readOnly && !adding && kinds.length > 0 ? (
        <Button
          variant="outline"
          disabled={!interactive || !active}
          onPress={() => setAdding(true)}
          testID="bot-channel-add"
        >
          {t("bots.addChannel")}
        </Button>
      ) : null}
    </SettingsGroup>
  );
}

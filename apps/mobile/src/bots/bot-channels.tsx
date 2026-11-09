import React, { useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { randomUUID } from "expo-crypto";
import type { z } from "zod";
import type { BotChannelSchema } from "@ait/protocol/bot/types";
import { Button } from "@/components/ui/button";
import { Field, FormTextInput } from "@/components/ui/form-field";
import { SelectField } from "@/components/ui/select-field";
import { Switch } from "@/components/ui/switch";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { confirmDialog } from "@/utils/confirm-dialog";
import { AllowGroupsCheckbox } from "./allow-groups-checkbox";
import { FeishuManualGuide, FeishuScanSetup, useFeishuSetupSupported } from "./feishu-setup";
import { TextLink } from "./channel-setup";
import {
  ChannelScanSetup,
  QqManualSetup,
  QqPairing,
  useQqSetupSupported,
  useWeixinSetupSupported,
} from "./scan-setup";
import { ImessageGuidedSetup, useImessageSetupSupported } from "./imessage-setup";
import { BotLoadingSpinner } from "./loading";
import { botKeys } from "./queries";
import { botStyles as styles } from "./styles";
import { TelegramGuidedSetup, useTelegramSetupSupported } from "./telegram-setup";

type BotChannel = z.infer<typeof BotChannelSchema>;

const CREDENTIAL_REF = /^env:AIT_SERVER_CREDENTIAL_[A-Z0-9_]{1,96}$/;
const MAX_SENDERS = 256;
type ExternalKind = "telegram" | "feishu" | "weixin" | "qq" | "photon_imessage";
const KIND_LABELS = {
  telegram: "bots.telegram",
  feishu: "bots.feishu",
  weixin: "bots.weixin",
  qq: "bots.qq",
  photon_imessage: "bots.imessage",
} as const;

function parseSenders(text: string): string[] {
  return text
    .split(/[,\n]/)
    .map((sender) => sender.trim())
    .filter(Boolean);
}

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
}: {
  serverId: string;
  workspaceId: string;
  builtinChannelId: string;
  channel: BotChannel;
  interactive: boolean;
  readOnly: boolean;
  active: boolean;
  onChanged: () => void;
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
  // Senders scanned in with QQ may have a different ID in groups, so groups need pairing too.
  const pairable =
    channel.config.kind === "qq" &&
    (channel.config.allowGroups || channel.config.allowedSenders.length === 0);
  const connection = status.data
    ? connectionSummary(
        status.data,
        channel.enabled ? t("bots.channelConnected") : t("bots.channelIdle"),
      )
    : null;
  return (
    <View style={styles.card} testID={`bot-channel-${channel.id}`}>
      <View style={styles.row}>
        <View style={styles.grow}>
          <Text style={styles.text}>{kind}</Text>
          {connection ? <Text style={styles.hint}>{connection}</Text> : null}
        </View>
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
            disabled={!interactive || busy}
            onPress={() => remove.mutate()}
            testID={`bot-channel-delete-${channel.id}`}
          >
            {t("bots.deleteChannel")}
          </Button>
        ) : null}
      </View>
      {pairable && !readOnly ? (
        <QqPairing
          serverId={serverId}
          workspaceId={workspaceId}
          channelId={channel.id}
          disabled={!interactive || !active || !channel.enabled}
        />
      ) : null}
      {toggle.error ? <Text style={styles.error}>{toggle.error.message}</Text> : null}
      {remove.error ? <Text style={styles.error}>{remove.error.message}</Text> : null}
    </View>
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
  const feishuSetupSupported = useFeishuSetupSupported(serverId);
  const [kind, setKind] = useState<ExternalKind>(
    feishuSetupSupported && kinds.includes("feishu") ? "feishu" : (kinds[0] ?? "telegram"),
  );
  const [tokenRef, setTokenRef] = useState("");
  const [appId, setAppId] = useState("");
  const [secretRef, setSecretRef] = useState("");
  const [senders, setSenders] = useState("");
  const [allowGroups, setAllowGroups] = useState(false);
  const [enterManually, setEnterManually] = useState(false);
  const scanning = kind === "feishu" && feishuSetupSupported && !enterManually;
  const telegramSetupSupported = useTelegramSetupSupported(serverId);
  const [useEnvironment, setUseEnvironment] = useState(false);
  const [enterQqManually, setEnterQqManually] = useState(false);
  const telegramGuided = kind === "telegram" && telegramSetupSupported && !useEnvironment;
  // Hosts with Feishu setup also accept a pasted App Secret; a reference still uses the variable.
  const pastedSecret = kind === "feishu" && feishuSetupSupported && !CREDENTIAL_REF.test(secretRef);
  const allowedSenders = parseSenders(senders);
  const valid =
    allowedSenders.length > 0 &&
    allowedSenders.length <= MAX_SENDERS &&
    (kind === "telegram"
      ? CREDENTIAL_REF.test(tokenRef)
      : appId.trim().length > 0 &&
        (CREDENTIAL_REF.test(secretRef) || (pastedSecret && secretRef.trim().length > 0)));
  const create = useMutation({
    mutationFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      // These render their own setup and never reach this form's submit button.
      if (kind === "weixin" || kind === "qq" || kind === "photon_imessage") return;
      if (pastedSecret) {
        await client.botRequest("bot.channel.feishu.connect.request", {
          workspaceId,
          channelId,
          appId: appId.trim(),
          appSecret: secretRef.trim(),
          allowedSenders,
          allowGroups,
        });
        return;
      }
      await client.botRequest("bot.channel.create.request", {
        workspaceId,
        channelId,
        enabled: false,
        config:
          kind === "telegram"
            ? { kind, tokenRef, allowedSenders, allowGroups }
            : { kind, appId: appId.trim(), secretRef, allowedSenders, allowGroups },
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
  if (kind === "weixin" || (kind === "qq" && !enterQqManually)) {
    return (
      <View style={styles.card} testID="bot-channel-form">
        {typeField}
        <ChannelScanSetup
          key={kind}
          kind={kind}
          serverId={serverId}
          workspaceId={workspaceId}
          active={active}
          onDone={onDone}
          onEnterManually={kind === "qq" ? () => setEnterQqManually(true) : undefined}
        />
      </View>
    );
  }
  if (kind === "qq") {
    return (
      <View style={styles.card} testID="bot-channel-form">
        {typeField}
        <QqManualSetup
          serverId={serverId}
          workspaceId={workspaceId}
          channelId={channelId}
          active={active}
          onDone={onDone}
          onScanInstead={() => setEnterQqManually(false)}
        />
      </View>
    );
  }
  if (kind === "photon_imessage") {
    return (
      <View style={styles.card} testID="bot-channel-form">
        {typeField}
        <ImessageGuidedSetup
          serverId={serverId}
          workspaceId={workspaceId}
          active={active}
          onDone={onDone}
        />
      </View>
    );
  }
  if (scanning) {
    return (
      <View style={styles.card} testID="bot-channel-form">
        {typeField}
        <FeishuScanSetup
          serverId={serverId}
          workspaceId={workspaceId}
          active={active}
          onDone={onDone}
          onEnterManually={() => setEnterManually(true)}
        />
      </View>
    );
  }
  if (telegramGuided) {
    return (
      <View style={styles.card} testID="bot-channel-form">
        {typeField}
        <TelegramGuidedSetup
          serverId={serverId}
          workspaceId={workspaceId}
          active={active}
          onDone={onDone}
          onUseEnvironment={() => setUseEnvironment(true)}
        />
      </View>
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
      {kind === "feishu" && feishuSetupSupported ? (
        <FeishuManualGuide onScanInstead={() => setEnterManually(false)} />
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
            label={feishuSetupSupported ? t("bots.feishuAppSecret") : t("bots.secretRef")}
            hint={feishuSetupSupported ? t("bots.feishuAppSecretHint") : t("bots.channelRefHint")}
          >
            <FormTextInput
              initialValue={secretRef}
              onChangeText={setSecretRef}
              editable={!create.isPending}
              secureTextEntry={feishuSetupSupported}
              accessibilityLabel={
                feishuSetupSupported ? t("bots.feishuAppSecret") : t("bots.secretRef")
              }
              autoCapitalize="none"
              autoCorrect={false}
            />
          </Field>
        </>
      )}
      {showHint ? <Text style={styles.error}>{t("bots.channelRefHint")}</Text> : null}
      <Field label={t("bots.allowedSenders")}>
        <FormTextInput
          multiline
          initialValue={senders}
          onChangeText={setSenders}
          editable={!create.isPending}
          accessibilityLabel={t("bots.allowedSenders")}
          autoCapitalize="none"
          autoCorrect={false}
        />
      </Field>
      <AllowGroupsCheckbox
        checked={allowGroups}
        disabled={create.isPending}
        onToggle={() => setAllowGroups((checked) => !checked)}
      />
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
  const weixinSupported = useWeixinSetupSupported(serverId);
  const qqSupported = useQqSetupSupported(serverId);
  const imessageSupported = useImessageSetupSupported(serverId);
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
  // Older clients reject unknown values in `supported`, so hosts advertise these by capability.
  const kinds: ExternalKind[] = types.data
    ? [
        ...types.data.filter(
          (kind): kind is "telegram" | "feishu" => kind === "telegram" || kind === "feishu",
        ),
        ...(weixinSupported ? (["weixin"] as const) : []),
        ...(qqSupported ? (["qq"] as const) : []),
        ...(imessageSupported ? (["photon_imessage"] as const) : []),
      ]
    : [];
  return (
    <View style={styles.card} testID="bot-channels">
      <Text style={styles.title}>{t("bots.channels")}</Text>
      {list.isPending && enabled ? <BotLoadingSpinner /> : null}
      {list.error ? <Text style={styles.error}>{list.error.message}</Text> : null}
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
    </View>
  );
}

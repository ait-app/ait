import React, { useState } from "react";
import { Pressable, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { randomUUID } from "expo-crypto";
import { Check } from "lucide-react-native";
import { withUnistyles } from "react-native-unistyles";
import type { z } from "zod";
import type { BotChannelSchema } from "@ait/protocol/bot/types";
import { Button } from "@/components/ui/button";
import { Field, FormTextInput } from "@/components/ui/form-field";
import { SelectField } from "@/components/ui/select-field";
import { Switch } from "@/components/ui/switch";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import type { Theme } from "@/styles/theme";
import { confirmDialog } from "@/utils/confirm-dialog";
import { BotLoadingSpinner } from "./loading";
import { botKeys } from "./queries";
import { botStyles as styles } from "./styles";

type BotChannel = z.infer<typeof BotChannelSchema>;
const ThemedCheck = withUnistyles(Check);
const checkedIconMapping = (theme: Theme) => ({ color: theme.colors.accentForeground });

const CREDENTIAL_REF = /^env:AIT_SERVER_CREDENTIAL_[A-Z0-9_]{1,96}$/;
const MAX_SENDERS = 256;
type ExternalKind = "telegram" | "feishu";

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
  onChanged,
}: {
  serverId: string;
  workspaceId: string;
  builtinChannelId: string;
  channel: BotChannel;
  interactive: boolean;
  readOnly: boolean;
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
  const kind = channel.config.kind === "feishu" ? t("bots.feishu") : t("bots.telegram");
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
          disabled={readOnly || !interactive || busy}
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
      {toggle.error ? <Text style={styles.error}>{toggle.error.message}</Text> : null}
      {remove.error ? <Text style={styles.error}>{remove.error.message}</Text> : null}
    </View>
  );
}

function ChannelForm({
  serverId,
  workspaceId,
  kinds,
  onDone,
}: {
  serverId: string;
  workspaceId: string;
  kinds: ExternalKind[];
  onDone: () => void;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const [kind, setKind] = useState<ExternalKind>(kinds[0] ?? "telegram");
  const [tokenRef, setTokenRef] = useState("");
  const [appId, setAppId] = useState("");
  const [secretRef, setSecretRef] = useState("");
  const [senders, setSenders] = useState("");
  const [allowGroups, setAllowGroups] = useState(false);
  const allowedSenders = parseSenders(senders);
  const valid =
    allowedSenders.length > 0 &&
    allowedSenders.length <= MAX_SENDERS &&
    (kind === "telegram"
      ? CREDENTIAL_REF.test(tokenRef)
      : appId.trim().length > 0 && CREDENTIAL_REF.test(secretRef));
  const create = useMutation({
    mutationFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      await client.botRequest("bot.channel.create.request", {
        workspaceId,
        channelId: randomUUID(),
        enabled: false,
        config:
          kind === "telegram"
            ? { kind, tokenRef, allowedSenders, allowGroups }
            : { kind, appId: appId.trim(), secretRef, allowedSenders, allowGroups },
      });
    },
    onSuccess: onDone,
  });
  const showHint = (kind === "telegram" ? tokenRef : secretRef) !== "" && !valid;
  return (
    <View style={styles.card} testID="bot-channel-form">
      <SelectField
        label={t("bots.channelType")}
        value={kind}
        selectedDisplay={{ label: kind === "feishu" ? t("bots.feishu") : t("bots.telegram") }}
        options={kinds.map((id) => ({
          id,
          value: id,
          label: id === "feishu" ? t("bots.feishu") : t("bots.telegram"),
        }))}
        onChange={(value) => setKind(value as ExternalKind)}
        disabled={create.isPending}
        placeholder={t("bots.channelType")}
        emptyText={t("bots.channelType")}
        testID="bot-channel-type"
      />
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
          <Field label={t("bots.secretRef")} hint={t("bots.channelRefHint")}>
            <FormTextInput
              initialValue={secretRef}
              onChangeText={setSecretRef}
              editable={!create.isPending}
              accessibilityLabel={t("bots.secretRef")}
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
      <Pressable
        style={[styles.row, create.isPending ? styles.disabled : null]}
        onPress={() => setAllowGroups((checked) => !checked)}
        disabled={create.isPending}
        accessibilityRole="checkbox"
        accessibilityLabel={t("bots.allowGroups")}
        accessibilityState={{ checked: allowGroups, disabled: create.isPending }}
        aria-checked={allowGroups}
        testID="bot-channel-allow-groups"
      >
        <View style={[styles.checkbox, allowGroups ? styles.checkboxChecked : null]}>
          {allowGroups ? <ThemedCheck size={14} uniProps={checkedIconMapping} /> : null}
        </View>
        <Text style={styles.text}>{t("bots.allowGroups")}</Text>
      </Pressable>
      {create.error ? (
        <Text style={styles.error} accessibilityRole="alert">
          {create.error.message}
        </Text>
      ) : null}
      <View style={styles.row}>
        <Button
          onPress={() => create.mutate()}
          disabled={!valid || create.isPending}
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
}: {
  serverId: string;
  workspaceId: string;
  builtinChannelId: string;
  interactive: boolean;
  readOnly: boolean;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const queryClient = useQueryClient();
  const [adding, setAdding] = useState(false);
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
  const kinds = (types.data ?? []).filter((kind): kind is ExternalKind => kind !== "builtin");
  return (
    <View style={styles.card} testID="bot-channels">
      <Text style={styles.title}>{t("bots.channels")}</Text>
      {list.isPending && enabled ? <BotLoadingSpinner /> : null}
      {list.error ? <Text style={styles.error}>{list.error.message}</Text> : null}
      {external.map((channel) => (
        <ChannelRow
          key={channel.id}
          serverId={serverId}
          workspaceId={workspaceId}
          builtinChannelId={builtinChannelId}
          channel={channel}
          interactive={interactive}
          readOnly={readOnly}
          onChanged={refresh}
        />
      ))}
      {!readOnly && adding && kinds.length > 0 ? (
        <ChannelForm
          serverId={serverId}
          workspaceId={workspaceId}
          kinds={kinds}
          onDone={() => {
            setAdding(false);
            void refresh();
          }}
        />
      ) : null}
      {!readOnly && !adding && kinds.length > 0 ? (
        <Button
          variant="outline"
          disabled={!interactive}
          onPress={() => setAdding(true)}
          testID="bot-channel-add"
        >
          {t("bots.addChannel")}
        </Button>
      ) : null}
    </View>
  );
}

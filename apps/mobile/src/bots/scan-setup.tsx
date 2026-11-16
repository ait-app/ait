import React, { useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { useMutation } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Field, FormTextInput } from "@/components/ui/form-field";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { openExternalUrl } from "@/utils/open-external-url";
import { AllowGroupsCheckbox } from "./allow-groups-checkbox";
import { TextLink, useLiveSetup } from "./channel-setup";
import { QrCode } from "./qr-code";
import { botStyles as styles } from "./styles";

export type ScanKind = "weixin" | "qq";

const QR_SIZE = 240;
const METHODS = {
  weixin: {
    start: "bot.channel.weixin.scan.start.request",
    status: "bot.channel.weixin.scan.status.request",
    cancel: "bot.channel.weixin.scan.cancel.request",
  },
  qq: {
    start: "bot.channel.qq.scan.start.request",
    status: "bot.channel.qq.scan.status.request",
    cancel: "bot.channel.qq.scan.cancel.request",
  },
} as const;
const COPY = {
  weixin: {
    hint: "bots.weixinScanHint",
    waiting: "bots.weixinScanWaiting",
    open: "bots.weixinScanOpen",
    qrLabel: "bots.weixinScanQrLabel",
    unavailable: "bots.weixinScanUnavailable",
  },
  qq: {
    hint: "bots.qqScanHint",
    waiting: "bots.qqScanWaiting",
    open: "bots.qqScanOpen",
    qrLabel: "bots.qqScanQrLabel",
    unavailable: "bots.qqScanUnavailable",
  },
} as const;

export function ChannelScanSetup({
  kind,
  serverId,
  workspaceId,
  active,
  onDone,
  onEnterManually,
}: {
  kind: ScanKind;
  serverId: string;
  workspaceId: string;
  active: boolean;
  onDone: () => void;
  /** Absent when the platform issues credentials only through scanning. */
  onEnterManually?: () => void;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const [allowGroups, setAllowGroups] = useState(false);
  const [scan, setScan] = useState<{ scanId: string; url: string } | null>(null);
  const copy = COPY[kind];
  const methods = METHODS[kind];
  const start = useMutation({
    mutationFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      return client.botRequest(methods.start, {
        workspaceId,
        allowGroups: kind === "qq" ? allowGroups : undefined,
      });
    },
    onSuccess: ({ scanId, url }) => setScan({ scanId, url }),
  });
  const { status, stop } = useLiveSetup({
    queryKey: ["bot-channel-scan", kind, serverId, workspaceId],
    sessionId: scan?.scanId,
    getStatus: async (scanId) => {
      if (!client) throw new Error(t("bots.disconnected"));
      return client.botRequest(methods.status, { workspaceId, scanId });
    },
    cancel: (scanId) => {
      void client?.botRequest(methods.cancel, { workspaceId, scanId }).catch(() => undefined);
    },
    onDone,
  });
  const cancelScan = () => {
    stop();
    setScan(null);
  };
  const manualLink = onEnterManually ? (
    <TextLink
      label={t("bots.feishuManualEntry")}
      onPress={onEnterManually}
      testID="bot-scan-manual"
    />
  ) : null;

  if (scan && status?.state === "failed") {
    const failure =
      status.reason === "expired"
        ? t("bots.feishuScanExpired")
        : status.reason === "alreadyConnected"
          ? t("bots.scanAlreadyConnected")
          : t(copy.unavailable);
    return (
      <View style={styles.permissionBlock}>
        <Text style={styles.error} accessibilityRole="alert" testID="bot-scan-failed">
          {failure}
        </Text>
        <View style={styles.row}>
          <Button
            onPress={() => {
              setScan(null);
              start.mutate();
            }}
            disabled={!active || start.isPending}
            testID="bot-scan-retry"
          >
            {t("bots.feishuScanRetry")}
          </Button>
          <Button variant="outline" onPress={onDone}>
            {t("common.actions.cancel")}
          </Button>
        </View>
        {manualLink}
      </View>
    );
  }
  if (scan) {
    return (
      <View style={styles.permissionBlock}>
        <QrCode
          value={scan.url}
          size={QR_SIZE}
          accessibilityLabel={t(copy.qrLabel)}
          testID="bot-scan-qr"
        />
        <Text style={styles.hint}>{t(copy.waiting)}</Text>
        <View style={styles.row}>
          <Button onPress={() => void openExternalUrl(scan.url)} testID="bot-scan-open">
            {t(copy.open)}
          </Button>
          <Button variant="outline" onPress={cancelScan} testID="bot-scan-cancel">
            {t("common.actions.cancel")}
          </Button>
        </View>
      </View>
    );
  }
  return (
    <View style={styles.permissionBlock}>
      <Text style={styles.hint}>{t(copy.hint)}</Text>
      {kind === "qq" ? (
        <AllowGroupsCheckbox
          checked={allowGroups}
          disabled={start.isPending}
          onToggle={() => setAllowGroups((checked) => !checked)}
        />
      ) : null}
      {start.error ? (
        <Text style={styles.error} accessibilityRole="alert">
          {start.error.message}
        </Text>
      ) : null}
      <View style={styles.row}>
        <Button
          onPress={() => start.mutate()}
          disabled={!active || start.isPending}
          testID="bot-scan-start"
        >
          {t("bots.feishuScanStart")}
        </Button>
        <Button variant="outline" onPress={onDone} disabled={start.isPending}>
          {t("common.actions.cancel")}
        </Button>
      </View>
      {manualLink}
    </View>
  );
}

export function QqManualSetup({
  serverId,
  workspaceId,
  channelId,
  active,
  onDone,
  onScanInstead,
}: {
  serverId: string;
  workspaceId: string;
  channelId: string;
  active: boolean;
  onDone: () => void;
  onScanInstead: () => void;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const [appId, setAppId] = useState("");
  const [appSecret, setAppSecret] = useState("");
  const [allowGroups, setAllowGroups] = useState(false);
  const connect = useMutation({
    mutationFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      await client.botRequest("bot.channel.qq.connect.request", {
        workspaceId,
        channelId,
        appId: appId.trim(),
        appSecret: appSecret.trim(),
        allowGroups,
      });
    },
    onSuccess: onDone,
  });
  const valid = appId.trim().length > 0 && appSecret.trim().length > 0;
  return (
    <View style={styles.permissionBlock} testID="bot-qq-manual">
      <Text style={styles.hint}>{`1. ${t("bots.qqManualStep1")}`}</Text>
      <Text style={styles.hint}>{`2. ${t("bots.qqManualStep2")}`}</Text>
      <TextLink
        label={t("bots.feishuScanInstead")}
        onPress={onScanInstead}
        testID="bot-qq-scan-instead"
      />
      <Field label={t("bots.appId")}>
        <FormTextInput
          initialValue={appId}
          onChangeText={setAppId}
          editable={!connect.isPending}
          accessibilityLabel={t("bots.appId")}
          autoCapitalize="none"
          autoCorrect={false}
        />
      </Field>
      <Field label={t("bots.feishuAppSecret")} hint={t("bots.qqAppSecretHint")}>
        <FormTextInput
          initialValue={appSecret}
          onChangeText={setAppSecret}
          editable={!connect.isPending}
          secureTextEntry
          accessibilityLabel={t("bots.feishuAppSecret")}
          autoCapitalize="none"
          autoCorrect={false}
        />
      </Field>
      <AllowGroupsCheckbox
        checked={allowGroups}
        disabled={connect.isPending}
        onToggle={() => setAllowGroups((checked) => !checked)}
      />
      {connect.error ? (
        <Text style={styles.error} accessibilityRole="alert">
          {connect.error.message}
        </Text>
      ) : null}
      <View style={styles.row}>
        <Button
          onPress={() => connect.mutate()}
          disabled={!valid || !active || connect.isPending}
          testID="bot-qq-connect"
        >
          {t("bots.addChannel")}
        </Button>
        <Button variant="outline" onPress={onDone} disabled={connect.isPending}>
          {t("common.actions.cancel")}
        </Button>
      </View>
    </View>
  );
}

/** Issue the one-time code that adds a sender to a QQ channel. */
export function QqPairing({
  serverId,
  workspaceId,
  channelId,
  disabled,
}: {
  serverId: string;
  workspaceId: string;
  channelId: string;
  disabled: boolean;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const pair = useMutation({
    mutationFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      return client.botRequest("bot.channel.qq.pair.request", { workspaceId, channelId });
    },
  });
  return (
    <View style={styles.permissionBlock}>
      <Button
        variant="outline"
        disabled={disabled || pair.isPending}
        onPress={() => pair.mutate()}
        testID={`bot-qq-pair-${channelId}`}
      >
        {t("bots.qqPairButton")}
      </Button>
      {pair.data ? (
        <Text style={styles.hint} selectable testID={`bot-qq-pair-code-${channelId}`}>
          {t("bots.qqPairHint", { code: pair.data.code })}
        </Text>
      ) : null}
      {pair.error ? <Text style={styles.error}>{pair.error.message}</Text> : null}
    </View>
  );
}

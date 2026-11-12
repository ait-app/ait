import React, { useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { useMutation } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { useSessionStore } from "@/stores/session-store";
import { openExternalUrl } from "@/utils/open-external-url";
import { AllowGroupsCheckbox } from "./allow-groups-checkbox";
import { TextLink, useLiveSetup } from "./channel-setup";
import { QrCode } from "./qr-code";
import { botStyles as styles } from "./styles";

const QR_SIZE = 240;
const FAILURE_KEYS = {
  denied: "bots.feishuScanDenied",
  expired: "bots.feishuScanExpired",
  larkUnsupported: "bots.feishuScanLark",
  unavailable: "bots.feishuScanUnavailable",
} as const;
const MANUAL_STEP_KEYS = [
  "bots.feishuManualStep1",
  "bots.feishuManualStep2",
  "bots.feishuManualStep3",
  "bots.feishuManualStep4",
  "bots.feishuManualStep5",
] as const;

/** Whether the host can run Feishu scan setup and store pasted secrets. */
export function useFeishuSetupSupported(serverId: string): boolean {
  return useSessionStore(
    (state) => state.sessions[serverId]?.serverInfo?.features?.workspaceBotFeishuSetup === true,
  );
}

/** Whether the host can verify and store credentials for Lark international. */
export function useLarkSetupSupported(serverId: string): boolean {
  return useSessionStore(
    (state) => state.sessions[serverId]?.serverInfo?.features?.workspaceBotLarkSetup === true,
  );
}

export function FeishuScanSetup({
  serverId,
  workspaceId,
  active,
  onDone,
  onEnterManually,
}: {
  serverId: string;
  workspaceId: string;
  active: boolean;
  onDone: () => void;
  onEnterManually: () => void;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const [allowGroups, setAllowGroups] = useState(false);
  const [scan, setScan] = useState<{ scanId: string; url: string } | null>(null);
  const start = useMutation({
    mutationFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      return client.botRequest("bot.channel.feishu.scan.start.request", {
        workspaceId,
        allowGroups,
      });
    },
    onSuccess: ({ scanId, url }) => setScan({ scanId, url }),
  });
  const { status, stop } = useLiveSetup({
    queryKey: ["bot-feishu-scan", serverId, workspaceId],
    sessionId: scan?.scanId,
    getStatus: async (scanId) => {
      if (!client) throw new Error(t("bots.disconnected"));
      return client.botRequest("bot.channel.feishu.scan.status.request", { workspaceId, scanId });
    },
    cancel: (scanId) => {
      void client
        ?.botRequest("bot.channel.feishu.scan.cancel.request", { workspaceId, scanId })
        .catch(() => undefined);
    },
    onDone,
  });
  const state = status?.state;
  const cancelScan = () => {
    stop();
    setScan(null);
  };
  const manualLink = (
    <TextLink
      label={t("bots.feishuManualEntry")}
      onPress={onEnterManually}
      testID="bot-feishu-manual"
    />
  );

  if (scan && state === "failed") {
    return (
      <View style={styles.permissionBlock}>
        <Text style={styles.error} accessibilityRole="alert" testID="bot-feishu-scan-failed">
          {t(FAILURE_KEYS[status?.reason ?? "unavailable"])}
        </Text>
        <View style={styles.row}>
          <Button
            onPress={() => {
              setScan(null);
              start.mutate();
            }}
            disabled={!active || start.isPending}
            testID="bot-feishu-scan-retry"
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
          accessibilityLabel={t("bots.feishuScanQrLabel")}
          testID="bot-feishu-scan-qr"
        />
        <Text style={styles.hint}>{t("bots.feishuScanWaiting")}</Text>
        <View style={styles.row}>
          <Button onPress={() => void openExternalUrl(scan.url)} testID="bot-feishu-scan-open">
            {t("bots.feishuScanOpen")}
          </Button>
          <Button variant="outline" onPress={cancelScan} testID="bot-feishu-scan-cancel">
            {t("common.actions.cancel")}
          </Button>
        </View>
      </View>
    );
  }
  return (
    <View style={styles.permissionBlock}>
      <Text style={styles.hint}>{t("bots.feishuScanHint")}</Text>
      <AllowGroupsCheckbox
        checked={allowGroups}
        disabled={start.isPending}
        onToggle={() => setAllowGroups((checked) => !checked)}
      />
      {start.error ? (
        <Text style={styles.error} accessibilityRole="alert">
          {start.error.message}
        </Text>
      ) : null}
      <View style={styles.row}>
        <Button
          onPress={() => start.mutate()}
          disabled={!active || start.isPending}
          testID="bot-feishu-scan-start"
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

export function FeishuManualGuide({
  platform = "feishu",
  onScanInstead,
}: {
  platform?: "feishu" | "lark";
  onScanInstead?: () => void;
}) {
  const { t } = useTranslation();
  const values = {
    platform: t(platform === "lark" ? "bots.lark" : "bots.feishu"),
    developerUrl: platform === "lark" ? "open.larksuite.com/app" : "open.feishu.cn/app",
  };
  return (
    <View style={styles.permissionBlock} testID={`bot-${platform}-manual-guide`}>
      {MANUAL_STEP_KEYS.map((key, index) => (
        <Text key={key} style={styles.hint}>
          {`${index + 1}. ${t(key, values)}`}
        </Text>
      ))}
      {onScanInstead ? (
        <TextLink
          label={t("bots.feishuScanInstead")}
          onPress={onScanInstead}
          testID="bot-feishu-scan-instead"
        />
      ) : null}
    </View>
  );
}

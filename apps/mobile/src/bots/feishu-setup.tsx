import React, { useEffect, useRef, useState } from "react";
import { Pressable, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { useSessionStore } from "@/stores/session-store";
import { openExternalUrl } from "@/utils/open-external-url";
import { AllowGroupsCheckbox } from "./allow-groups-checkbox";
import { QrCode } from "./qr-code";
import { botStyles as styles } from "./styles";

const STATUS_POLL_MS = 3000;
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

function TextLink({
  label,
  onPress,
  testID,
}: {
  label: string;
  onPress: () => void;
  testID: string;
}) {
  return (
    <Pressable onPress={onPress} accessibilityRole="button" testID={testID}>
      <Text style={[styles.hint, styles.link]}>{label}</Text>
    </Pressable>
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
  // Leaving a live scan must cancel it, or a later approval would create an app with no channel.
  const cancelLiveScan = useRef<(() => void) | null>(null);
  const start = useMutation({
    mutationFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      return client.botRequest("bot.channel.feishu.scan.start.request", {
        workspaceId,
        allowGroups,
      });
    },
    onSuccess: ({ scanId, url }) => {
      cancelLiveScan.current = () => {
        void client
          ?.botRequest("bot.channel.feishu.scan.cancel.request", { workspaceId, scanId })
          .catch(() => undefined);
      };
      setScan({ scanId, url });
    },
  });
  const scanId = scan?.scanId;
  const status = useQuery({
    queryKey: ["bot-feishu-scan", serverId, workspaceId, scanId],
    enabled: Boolean(client && scanId),
    gcTime: 0,
    refetchInterval: (query) =>
      query.state.data && query.state.data.state !== "waiting" ? false : STATUS_POLL_MS,
    queryFn: async () => {
      if (!client || !scanId) throw new Error(t("bots.disconnected"));
      return client.botRequest("bot.channel.feishu.scan.status.request", { workspaceId, scanId });
    },
  });
  const state = status.data?.state;
  useEffect(() => {
    if (state !== "complete" && state !== "failed") return;
    cancelLiveScan.current = null;
    if (state === "complete") onDone();
  }, [state, onDone]);
  useEffect(() => () => cancelLiveScan.current?.(), []);
  const cancelScan = () => {
    cancelLiveScan.current?.();
    cancelLiveScan.current = null;
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
          {t(FAILURE_KEYS[status.data?.reason ?? "unavailable"])}
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

export function FeishuManualGuide({ onScanInstead }: { onScanInstead: () => void }) {
  const { t } = useTranslation();
  return (
    <View style={styles.permissionBlock} testID="bot-feishu-manual-guide">
      {MANUAL_STEP_KEYS.map((key, index) => (
        <Text key={key} style={styles.hint}>
          {`${index + 1}. ${t(key)}`}
        </Text>
      ))}
      <TextLink
        label={t("bots.feishuScanInstead")}
        onPress={onScanInstead}
        testID="bot-feishu-scan-instead"
      />
    </View>
  );
}

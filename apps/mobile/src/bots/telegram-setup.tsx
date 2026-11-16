import React, { useEffect, useState } from "react";
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

const BOT_FATHER_URL = "https://t.me/BotFather";
const BOT_FATHER_QR_SIZE = 160;
const PAIRING_QR_SIZE = 240;
const GUIDE_STEP_KEYS = [
  "bots.telegramGuideStep1",
  "bots.telegramGuideStep2",
  "bots.telegramGuideStep3",
] as const;
const FAILURE_KEYS = {
  invalidToken: "bots.telegramInvalidToken",
  alreadyConnected: "bots.telegramAlreadyConnected",
  webhookConflict: "bots.telegramWebhookConflict",
  pollingConflict: "bots.telegramPollingConflict",
  expired: "bots.telegramExpired",
  unavailable: "bots.telegramUnavailable",
} as const;
type Failure = keyof typeof FAILURE_KEYS;

export function TelegramGuidedSetup({
  serverId,
  workspaceId,
  active,
  onDone,
  onUseEnvironment,
}: {
  serverId: string;
  workspaceId: string;
  active: boolean;
  onDone: () => void;
  onUseEnvironment: () => void;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const [token, setToken] = useState("");
  const [allowGroups, setAllowGroups] = useState(false);
  const [failure, setFailure] = useState<Failure | null>(null);
  const [pairing, setPairing] = useState<{
    pairingId: string;
    url: string;
    botUsername: string;
  } | null>(null);
  const start = useMutation({
    mutationFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      return client.botRequest("bot.channel.telegram.pair.start.request", {
        workspaceId,
        token: token.trim(),
        allowGroups,
      });
    },
    onSuccess: (result) => {
      const { pairingId, url, botUsername } = result;
      if (result.state === "waiting" && pairingId && url && botUsername) {
        setFailure(null);
        setPairing({ pairingId, url, botUsername });
      } else {
        setFailure(result.reason ?? "unavailable");
      }
    },
  });
  const { status, stop } = useLiveSetup({
    queryKey: ["bot-telegram-pairing", serverId, workspaceId],
    sessionId: pairing?.pairingId,
    getStatus: async (pairingId) => {
      if (!client) throw new Error(t("bots.disconnected"));
      return client.botRequest("bot.channel.telegram.pair.status.request", {
        workspaceId,
        pairingId,
      });
    },
    cancel: (pairingId) => {
      void client
        ?.botRequest("bot.channel.telegram.pair.cancel.request", { workspaceId, pairingId })
        .catch(() => undefined);
    },
    onDone,
  });
  const failedReason = status?.state === "failed" ? (status.reason ?? "unavailable") : null;
  useEffect(() => {
    if (!failedReason) return;
    setFailure(failedReason);
    setPairing(null);
  }, [failedReason]);

  if (pairing) {
    return (
      <View style={styles.permissionBlock}>
        <QrCode
          value={pairing.url}
          size={PAIRING_QR_SIZE}
          accessibilityLabel={t("bots.telegramPairQrLabel")}
          testID="bot-telegram-pair-qr"
        />
        <Text style={styles.hint}>
          {t("bots.telegramPairWaiting", { bot: pairing.botUsername })}
        </Text>
        <View style={styles.row}>
          <Button onPress={() => void openExternalUrl(pairing.url)} testID="bot-telegram-pair-open">
            {t("bots.telegramPairOpen")}
          </Button>
          <Button
            variant="outline"
            onPress={() => {
              stop();
              setPairing(null);
            }}
            testID="bot-telegram-pair-cancel"
          >
            {t("common.actions.cancel")}
          </Button>
        </View>
      </View>
    );
  }
  return (
    <View style={styles.permissionBlock} testID="bot-telegram-guide">
      <QrCode
        value={BOT_FATHER_URL}
        size={BOT_FATHER_QR_SIZE}
        accessibilityLabel={t("bots.telegramBotFatherQrLabel")}
        testID="bot-telegram-botfather-qr"
      />
      <Button
        variant="outline"
        onPress={() => void openExternalUrl(BOT_FATHER_URL)}
        testID="bot-telegram-botfather-open"
      >
        {t("bots.telegramBotFatherOpen")}
      </Button>
      {GUIDE_STEP_KEYS.map((key, index) => (
        <Text key={key} style={styles.hint}>
          {`${index + 1}. ${t(key)}`}
        </Text>
      ))}
      <Field label={t("bots.telegramToken")} hint={t("bots.telegramTokenHint")}>
        <FormTextInput
          initialValue={token}
          onChangeText={setToken}
          editable={!start.isPending}
          secureTextEntry
          accessibilityLabel={t("bots.telegramToken")}
          autoCapitalize="none"
          autoCorrect={false}
        />
      </Field>
      <AllowGroupsCheckbox
        checked={allowGroups}
        disabled={start.isPending}
        onToggle={() => setAllowGroups((checked) => !checked)}
      />
      {failure ? (
        <Text style={styles.error} accessibilityRole="alert" testID="bot-telegram-failure">
          {t(FAILURE_KEYS[failure])}
        </Text>
      ) : null}
      {start.error ? (
        <Text style={styles.error} accessibilityRole="alert">
          {start.error.message}
        </Text>
      ) : null}
      <View style={styles.row}>
        <Button
          onPress={() => start.mutate()}
          disabled={!active || start.isPending || token.trim().length === 0}
          testID="bot-telegram-continue"
        >
          {t("bots.telegramContinue")}
        </Button>
        <Button variant="outline" onPress={onDone} disabled={start.isPending}>
          {t("common.actions.cancel")}
        </Button>
      </View>
      <TextLink
        label={t("bots.telegramUseEnvironment")}
        onPress={onUseEnvironment}
        testID="bot-telegram-use-environment"
      />
    </View>
  );
}

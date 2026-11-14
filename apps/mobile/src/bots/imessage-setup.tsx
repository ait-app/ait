import React, { useEffect, useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { useMutation } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Field, FormTextInput } from "@/components/ui/form-field";
import { getIsElectron } from "@/constants/platform";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { openExternalUrl, openSmsUrl } from "@/utils/open-external-url";
import { useLiveSetup } from "./channel-setup";
import { QrCode } from "./qr-code";
import { botStyles as styles } from "./styles";

const PHOTON_URL = "https://app.beta.photon.codes";
const PAIRING_QR_SIZE = 240;
const GUIDE_STEP_KEYS = [
  "bots.imessageGuideStep1",
  "bots.imessageGuideStep2",
  "bots.imessageGuideStep3",
] as const;
const FAILURE_KEYS = {
  invalidKey: "bots.imessageInvalidKey",
  invalidHandle: "bots.imessageInvalidHandle",
  dedicatedLine: "bots.imessageDedicatedLine",
  alreadyConnected: "bots.imessageAlreadyConnected",
  expired: "bots.imessageExpired",
  unavailable: "bots.imessageUnavailable",
} as const;
type Failure = keyof typeof FAILURE_KEYS;

export function ImessageGuidedSetup({
  serverId,
  workspaceId,
  active,
  onDone,
}: {
  serverId: string;
  workspaceId: string;
  active: boolean;
  onDone: () => void;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const [projectId, setProjectId] = useState("");
  // Read by the mutation below rather than passed to it, so the mutation cache never holds the key.
  const [apiKey, setApiKey] = useState("");
  const [handle, setHandle] = useState("");
  const [failure, setFailure] = useState<Failure | null>(null);
  const [pairing, setPairing] = useState<{
    pairingId: string;
    phoneNumber: string;
    code: string;
    url: string;
    expiresInSeconds: number;
    handle: string;
  } | null>(null);
  const start = useMutation({
    mutationFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      return client.botRequest("bot.channel.imessage.pair.start.request", {
        workspaceId,
        projectId: projectId.trim(),
        apiKey: apiKey.trim(),
        handle: handle.trim(),
      });
    },
    onSuccess: (result) => {
      const { pairingId, phoneNumber, code, url, expiresInSeconds } = result;
      if (
        result.state === "waiting" &&
        pairingId &&
        phoneNumber &&
        code &&
        url &&
        expiresInSeconds !== undefined
      ) {
        setFailure(null);
        setPairing({
          pairingId,
          phoneNumber,
          code,
          url,
          expiresInSeconds,
          handle: handle.trim(),
        });
      } else {
        setFailure(result.reason ?? "unavailable");
      }
    },
  });
  const { status, stop } = useLiveSetup({
    queryKey: ["bot-imessage-pairing", serverId, workspaceId],
    sessionId: pairing?.pairingId,
    getStatus: async (pairingId) => {
      if (!client) throw new Error(t("bots.disconnected"));
      return client.botRequest("bot.channel.imessage.pair.status.request", {
        workspaceId,
        pairingId,
      });
    },
    cancel: (pairingId) => {
      void client
        ?.botRequest("bot.channel.imessage.pair.cancel.request", { workspaceId, pairingId })
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
      <View style={styles.permissionBlock} testID="bot-imessage-pairing">
        <QrCode
          value={pairing.url}
          size={PAIRING_QR_SIZE}
          accessibilityLabel={t("bots.imessagePairQrLabel")}
          testID="bot-imessage-pair-qr"
        />
        <Text style={styles.hint}>{t("bots.imessagePairPhoneNumber")}</Text>
        <Text style={styles.text} selectable testID="bot-imessage-pair-number">
          {pairing.phoneNumber}
        </Text>
        <Text style={styles.hint}>{t("bots.imessagePairCode")}</Text>
        <Text style={styles.text} selectable testID="bot-imessage-pair-code">
          {pairing.code}
        </Text>
        <Text style={styles.hint} testID="bot-imessage-pair-waiting">
          {t("bots.imessagePairWaiting", { handle: pairing.handle })}
        </Text>
        <Text style={styles.hint}>
          {t("bots.imessagePairExpires", {
            minutes: Math.ceil(pairing.expiresInSeconds / 60),
          })}
        </Text>
        <View style={styles.row}>
          {/* The desktop opener only accepts web and mailto links, so desktop relies on the QR. */}
          {getIsElectron() ? null : (
            <Button
              // A device with no Messages app rejects this; the number and code stay on screen.
              onPress={() => void openSmsUrl(pairing.url).catch(() => undefined)}
              testID="bot-imessage-pair-open"
            >
              {t("bots.imessagePairOpen")}
            </Button>
          )}
          <Button
            variant="outline"
            onPress={() => {
              stop();
              setPairing(null);
            }}
            testID="bot-imessage-pair-cancel"
          >
            {t("common.actions.cancel")}
          </Button>
        </View>
      </View>
    );
  }
  const complete =
    projectId.trim().length > 0 && apiKey.trim().length > 0 && handle.trim().length > 0;
  return (
    <View style={styles.permissionBlock} testID="bot-imessage-guide">
      {GUIDE_STEP_KEYS.map((key, index) => (
        <Text key={key} style={styles.hint}>
          {`${index + 1}. ${t(key)}`}
        </Text>
      ))}
      <Button
        variant="outline"
        onPress={() => void openExternalUrl(PHOTON_URL)}
        testID="bot-imessage-photon-open"
      >
        {t("bots.imessagePhotonOpen")}
      </Button>
      <Text style={styles.hint} testID="bot-imessage-privacy">
        {t("bots.imessagePrivacyNotice")}
      </Text>
      <Field label={t("bots.imessageProjectId")}>
        <FormTextInput
          initialValue={projectId}
          onChangeText={setProjectId}
          editable={!start.isPending}
          accessibilityLabel={t("bots.imessageProjectId")}
          autoCapitalize="none"
          autoCorrect={false}
        />
      </Field>
      <Field label={t("bots.imessageApiKey")} hint={t("bots.imessageApiKeyHint")}>
        <FormTextInput
          initialValue={apiKey}
          onChangeText={setApiKey}
          editable={!start.isPending}
          secureTextEntry
          accessibilityLabel={t("bots.imessageApiKey")}
          autoCapitalize="none"
          autoCorrect={false}
          autoComplete="off"
          textContentType="none"
        />
      </Field>
      <Field label={t("bots.imessageHandle")} hint={t("bots.imessageHandleHint")}>
        <FormTextInput
          initialValue={handle}
          onChangeText={setHandle}
          editable={!start.isPending}
          accessibilityLabel={t("bots.imessageHandle")}
          autoCapitalize="none"
          autoCorrect={false}
        />
      </Field>
      {failure ? (
        <Text style={styles.error} accessibilityRole="alert" testID="bot-imessage-failure">
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
          disabled={!active || start.isPending || !complete}
          testID="bot-imessage-continue"
        >
          {t("bots.imessageContinue")}
        </Button>
        <Button variant="outline" onPress={onDone} disabled={start.isPending}>
          {t("common.actions.cancel")}
        </Button>
      </View>
    </View>
  );
}

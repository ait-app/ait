import React, { useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { useMutation } from "@tanstack/react-query";
import * as Clipboard from "expo-clipboard";
import { Button } from "@/components/ui/button";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { useSessionStore } from "@/stores/session-store";
import { confirmDialog } from "@/utils/confirm-dialog";
import { botStyles as styles } from "./styles";

const FAILURE_KEYS = {
  centerOffline: "bots.webhookCenterOffline",
  centerUnsupported: "bots.webhookCenterUnsupported",
  automationUnavailable: "bots.webhookAutomationUnavailable",
  limitReached: "bots.webhookLimitReached",
  centerError: "bots.webhookCenterError",
} as const;
type Failure = keyof typeof FAILURE_KEYS;

/** Whether the host can ask its Ait center for webhook channels. */
export function useWebhookSetupSupported(serverId: string): boolean {
  return useSessionStore(
    (state) => state.sessions[serverId]?.serverInfo?.features?.workspaceBotWebhookSetup === true,
  );
}

function curlExample(url: string, secret: string): string {
  return [
    `curl -X POST '${url}' \\`,
    `  -H 'Authorization: Bearer ${secret}' \\`,
    "  -H 'Content-Type: application/json' \\",
    `  -d '{"text":"Hello","thread":"demo"}'`,
  ].join("\n");
}

/** One value with a copy button; copying is how the owner takes it to the other system. */
export function CopyField({
  label,
  value,
  testID,
}: {
  label: string;
  value: string;
  testID: string;
}) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  return (
    <View style={styles.permissionBlock}>
      <View style={styles.row}>
        <Text style={[styles.hint, styles.grow]}>{label}</Text>
        <Button
          variant="outline"
          size="sm"
          onPress={() => {
            void Clipboard.setStringAsync(value).then(() => setCopied(true));
          }}
          accessibilityLabel={`${t("bots.webhookCopy")} ${label}`}
          testID={`${testID}-copy`}
        >
          {copied ? t("bots.webhookCopied") : t("bots.webhookCopy")}
        </Button>
      </View>
      <Text selectable style={styles.mono} testID={testID}>
        {value}
      </Text>
    </View>
  );
}

/** The endpoint with its secret, which the host cannot show again after this. */
export function WebhookSecret({
  url,
  secret,
  onDone,
}: {
  url: string;
  secret: string;
  onDone: () => void;
}) {
  const { t } = useTranslation();
  return (
    <View style={styles.permissionBlock} testID="bot-webhook-secret">
      <CopyField label={t("bots.webhookUrl")} value={url} testID="bot-webhook-url" />
      <CopyField label={t("bots.webhookSecret")} value={secret} testID="bot-webhook-secret-value" />
      <Text style={styles.warning}>{t("bots.webhookSecretOnce")}</Text>
      <CopyField
        label={t("bots.webhookExample")}
        value={curlExample(url, secret)}
        testID="bot-webhook-example"
      />
      <Text style={styles.hint}>{t("bots.webhookExampleHint")}</Text>
      <View style={styles.row}>
        <Button onPress={onDone} testID="bot-webhook-done">
          {t("bots.webhookDone")}
        </Button>
      </View>
    </View>
  );
}

export function WebhookSetup({
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
  const [failure, setFailure] = useState<Failure | null>(null);
  const [issued, setIssued] = useState<{
    url: string;
    secret: string;
  } | null>(null);
  const create = useMutation({
    mutationFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      return client.botRequest("bot.channel.webhook.create.request", {
        workspaceId,
      });
    },
    onSuccess: (result) => {
      const config = result.channel?.config;
      if (result.state === "created" && config?.kind === "webhook" && result.secret) {
        setFailure(null);
        setIssued({ url: config.url, secret: result.secret });
      } else {
        setFailure(result.reason ?? "centerError");
      }
    },
  });
  if (issued) return <WebhookSecret url={issued.url} secret={issued.secret} onDone={onDone} />;
  return (
    <View style={styles.permissionBlock} testID="bot-webhook-setup">
      <Text style={styles.hint}>{t("bots.webhookHint")}</Text>
      {failure ? (
        <Text style={styles.error} accessibilityRole="alert" testID="bot-webhook-failure">
          {t(FAILURE_KEYS[failure])}
        </Text>
      ) : null}
      {create.error ? (
        <Text style={styles.error} accessibilityRole="alert">
          {create.error.message}
        </Text>
      ) : null}
      <View style={styles.row}>
        <Button
          onPress={() => create.mutate()}
          disabled={!active || create.isPending}
          testID="bot-webhook-create"
        >
          {t("bots.webhookCreate")}
        </Button>
        <Button variant="outline" onPress={onDone} disabled={create.isPending}>
          {t("common.actions.cancel")}
        </Button>
      </View>
    </View>
  );
}

/** A webhook channel's URL, and a new secret on request. */
export function WebhookDetails({
  serverId,
  workspaceId,
  channelId,
  url,
  disabled,
}: {
  serverId: string;
  workspaceId: string;
  channelId: string;
  url: string;
  disabled: boolean;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const [failure, setFailure] = useState<Failure | null>(null);
  const [secret, setSecret] = useState<string | null>(null);
  const rotate = useMutation({
    mutationFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      if (
        !(await confirmDialog({
          title: t("bots.webhookRotate"),
          message: t("bots.webhookRotateConfirm"),
          confirmLabel: t("bots.webhookRotate"),
          cancelLabel: t("common.actions.cancel"),
          destructive: true,
        }))
      )
        return null;
      return client.botRequest("bot.channel.webhook.rotate.request", {
        workspaceId,
        channelId,
      });
    },
    onSuccess: (result) => {
      if (!result) return;
      if (result.state === "rotated" && result.secret) {
        setFailure(null);
        setSecret(result.secret);
      } else {
        setFailure(result.reason ?? "centerError");
      }
    },
  });
  if (secret) return <WebhookSecret url={url} secret={secret} onDone={() => setSecret(null)} />;
  return (
    <View style={styles.permissionBlock}>
      <CopyField label={t("bots.webhookUrl")} value={url} testID={`bot-webhook-url-${channelId}`} />
      {failure ? (
        <Text style={styles.error} accessibilityRole="alert">
          {t(FAILURE_KEYS[failure])}
        </Text>
      ) : null}
      {rotate.error ? <Text style={styles.error}>{rotate.error.message}</Text> : null}
      <View style={styles.row}>
        <Button
          variant="outline"
          onPress={() => rotate.mutate()}
          disabled={disabled || rotate.isPending}
          testID={`bot-webhook-rotate-${channelId}`}
        >
          {t("bots.webhookRotate")}
        </Button>
      </View>
    </View>
  );
}

import React from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  BOT_SKIP_REASONS,
  type BotLoggedNotification,
  type BotView,
  type BotWakeScope,
  type BotWakeStatus,
} from "@ait/protocol/bot/types";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { useSessionStore } from "@/stores/session-store";
import { formatMessageTimestamp } from "@/utils/time";
import { BotSelect } from "./bot-select";
import { NotificationKindIcon, notificationKindLabel } from "./bot-wake";
import { BotLoadingSpinner } from "./loading";
import { botKeys } from "./queries";
import { botStyles as styles } from "./styles";

/** Entries the settings screen shows; the log itself keeps more. */
const RECENT_LIMIT = 10;

/**
 * Whether the host implements Bot notifications (`bot.notifications.list.request` and the
 * `wakeScope` setting); older hosts hide both.
 */
export function useBotNotificationsSupported(serverId: string): boolean {
  return useSessionStore(
    (state) => state.sessions[serverId]?.serverInfo?.features?.workspaceBotNotifications === true,
  );
}

/** The wake-scope choice shared by setup and settings. */
export function BotWakeScopeField({
  value,
  onChange,
  disabled,
}: {
  value: BotWakeScope;
  onChange: (value: BotWakeScope) => void;
  disabled: boolean;
}) {
  const { t } = useTranslation();
  return (
    <>
      <BotSelect
        label={t("bots.notifications.wakeScope")}
        value={value}
        options={[
          { id: "own", label: t("bots.notifications.wakeScopeOwn") },
          { id: "all", label: t("bots.notifications.wakeScopeAll") },
        ]}
        onChange={(next) => onChange(next === "all" ? "all" : "own")}
        disabled={disabled}
        testID="bot-wake-scope"
      />
      <Text style={styles.hint}>{t("bots.notifications.wakeScopeHint")}</Text>
    </>
  );
}

/** Wake states and skip reasons with their own label; a newer host's others read generically. */
const LABELLED_STATES: ReadonlySet<string> = new Set(["logged", "pending", "woke"]);
const LABELLED_REASONS: ReadonlySet<string> = new Set(BOT_SKIP_REASONS);

function wakeStatusLabel(wake: BotWakeStatus, t: (key: string) => string): string {
  if (wake.state === "skipped") {
    const reason = "reason" in wake && typeof wake.reason === "string" ? wake.reason : "";
    return t(`bots.notifications.skipped.${LABELLED_REASONS.has(reason) ? reason : "other"}`);
  }
  return t(`bots.notifications.state.${LABELLED_STATES.has(wake.state) ? wake.state : "other"}`);
}

function RecentNotification({ entry }: { entry: BotLoggedNotification }) {
  const { t } = useTranslation();
  const { notification, wake } = entry;
  const at = new Date(notification.at);
  const time = Number.isNaN(at.getTime()) ? notification.at : formatMessageTimestamp(at);
  return (
    <View style={styles.permissionBlock} testID={`bot-notification-${entry.sequence}`}>
      <View style={styles.row}>
        <View accessibilityLabel={notificationKindLabel(notification.kind, t)} accessible>
          <NotificationKindIcon kind={notification.kind} />
        </View>
        <Text style={[styles.text, styles.grow]} numberOfLines={2}>
          {notification.title}
        </Text>
      </View>
      <Text style={wake.state === "skipped" ? styles.warning : styles.hint}>
        {`${time} · ${wakeStatusLabel(wake, t)}`}
      </Text>
    </View>
  );
}

/**
 * Wake scope and a short read-only list of recent notifications, which also shows the ones that
 * did not wake the Bot and why (budget, full inbox, pause, already seen, interrupted conversation).
 */
export function BotNotificationSettings({
  serverId,
  workspaceId,
  bot,
  interactive,
  readOnly,
}: {
  serverId: string;
  workspaceId: string;
  bot: BotView;
  interactive: boolean;
  readOnly: boolean;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const queryClient = useQueryClient();
  const supported = useBotNotificationsSupported(serverId);
  const wakeScope = bot.wakeScope;
  // A host that advertises the method but returns no wake scope cannot accept it either.
  const available = supported && wakeScope !== undefined;
  const recent = useQuery({
    queryKey: botKeys.notifications(serverId, workspaceId, bot.builtinChannelId),
    enabled: available && interactive && Boolean(client),
    staleTime: 0,
    refetchInterval: 15_000,
    queryFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      return (
        await client.botRequest("bot.notifications.list.request", {
          workspaceId,
          limit: RECENT_LIMIT,
        })
      ).notifications;
    },
  });
  const save = useMutation({
    mutationFn: async (next: BotWakeScope) => {
      if (!client) throw new Error(t("bots.disconnected"));
      // Hosts with notifications keep the execution when it is absent; resending the one read
      // here could revert a model change made on another device since.
      return (await client.botRequest("bot.update.request", { workspaceId, wakeScope: next })).bot;
    },
    onSuccess: (updated) => {
      queryClient.setQueryData(botKeys.bot(serverId, workspaceId), updated);
    },
  });
  if (!available) return null;
  return (
    <View style={styles.card} testID="bot-notifications-section">
      <Text style={styles.title}>{t("bots.notifications.title")}</Text>
      <BotWakeScopeField
        value={wakeScope}
        onChange={(next) => {
          if (next !== wakeScope) save.mutate(next);
        }}
        disabled={readOnly || !interactive || save.isPending}
      />
      {save.error ? <Text style={styles.error}>{save.error.message}</Text> : null}
      <Text style={styles.text}>{t("bots.notifications.recent")}</Text>
      {recent.isPending && interactive ? <BotLoadingSpinner /> : null}
      {recent.error ? <Text style={styles.error}>{recent.error.message}</Text> : null}
      {recent.data?.length === 0 ? (
        <Text style={styles.hint}>{t("bots.notifications.empty")}</Text>
      ) : null}
      {recent.data?.map((entry) => (
        <RecentNotification key={entry.sequence} entry={entry} />
      ))}
    </View>
  );
}

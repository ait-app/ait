import React from "react";
import { ScrollView, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import type { BotView } from "@ait/protocol/bot/types";
import { Button } from "@/components/ui/button";
import { canManageBackgroundTasks } from "@/hosts/background-task-access";
import { BotChat } from "./bot-chat";
import { BotSettings } from "./bot-settings";
import { BotSetup } from "./bot-setup";
import { BotLoadingSpinner } from "./loading";
import { useWorkspaceBot } from "./queries";
import { botStyles as styles } from "./styles";

export interface WorkspaceBotTabProps {
  serverId: string;
  workspaceId: string;
  cwd: string;
  view?: "settings";
  interactive: boolean;
  onChat: () => void;
  onSettings: () => void;
}

export function WorkspaceBotTab(props: WorkspaceBotTabProps) {
  const { serverId, workspaceId, view, interactive, onChat, onSettings } = props;
  const { t } = useTranslation();
  const detail = useWorkspaceBot(serverId, workspaceId, interactive);
  const bot = detail.data;
  // A failed refresh keeps the last Bot on screen, so report it in either view.
  const refreshError = detail.error ? (
    <View style={styles.card}>
      <Text style={styles.error} accessibilityRole="alert">
        {detail.error.message}
      </Text>
      <Button
        variant="outline"
        onPress={() => {
          void detail.refetch();
        }}
      >
        {t("bots.reload")}
      </Button>
    </View>
  ) : null;
  return (
    <View style={styles.root} testID="workspace-bot-tab">
      <View style={styles.toolbar}>
        <Text numberOfLines={1} style={[styles.title, styles.grow]}>
          {t("bots.title")}
        </Text>
        {bot ? (
          <>
            <Button
              variant={view === "settings" ? "ghost" : "outline"}
              size="sm"
              onPress={onChat}
              testID="bot-show-chat"
            >
              {t("bots.chat")}
            </Button>
            <Button
              variant={view === "settings" ? "outline" : "ghost"}
              size="sm"
              onPress={onSettings}
              testID="bot-show-settings"
            >
              {t("bots.settings")}
            </Button>
          </>
        ) : null}
      </View>
      {bot && view !== "settings" ? (
        <>
          {refreshError}
          <BotChat
            key={bot.builtinChannelId}
            serverId={serverId}
            workspaceId={workspaceId}
            bot={bot}
            interactive={interactive}
          />
        </>
      ) : (
        <ScrollView contentContainerStyle={styles.content}>
          <WorkspaceBotBody {...props} bot={bot} />
          {detail.isPending ? <BotLoadingSpinner /> : null}
          {refreshError}
        </ScrollView>
      )}
    </View>
  );
}

/** Settings of a configured Bot, or the setup wizard once the host reports no Bot. */
function WorkspaceBotBody({
  serverId,
  workspaceId,
  cwd,
  interactive,
  onChat,
  bot,
}: WorkspaceBotTabProps & { bot: BotView | null | undefined }) {
  const { t } = useTranslation();
  if (bot)
    return (
      <BotSettings
        key={bot.builtinChannelId}
        serverId={serverId}
        workspaceId={workspaceId}
        cwd={cwd}
        bot={bot}
        interactive={interactive}
        onRemoved={onChat}
      />
    );
  if (bot === undefined) return null;
  if (!canManageBackgroundTasks(serverId))
    return <Text style={styles.hint}>{t("bots.notConfigured")}</Text>;
  return (
    <BotSetup
      serverId={serverId}
      workspaceId={workspaceId}
      cwd={cwd}
      interactive={interactive}
      onConfigured={onChat}
    />
  );
}

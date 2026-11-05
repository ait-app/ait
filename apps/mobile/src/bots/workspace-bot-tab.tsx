import React from "react";
import { ScrollView, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { BotLoadingSpinner } from "./loading";
import { canManageBackgroundTasks } from "@/hosts/background-task-access";
import { BotForm } from "./bot-form";
import { BotChat } from "./bot-chat";
import { BotSettings } from "./bot-settings";
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
  const { serverId, workspaceId, cwd, view, interactive, onChat, onSettings } = props;
  const { t } = useTranslation();
  const readOnly = !canManageBackgroundTasks(serverId);
  const detail = useWorkspaceBot(serverId, workspaceId, interactive);
  const bot = detail.data ?? undefined;
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
            {!readOnly ? (
              <Button
                variant={view === "settings" ? "outline" : "ghost"}
                size="sm"
                onPress={onSettings}
                testID="bot-show-settings"
              >
                {t("bots.settings")}
              </Button>
            ) : null}
          </>
        ) : null}
      </View>
      {bot && view !== "settings" ? (
        <BotChat key={bot.id} serverId={serverId} bot={bot} interactive={interactive} />
      ) : (
        <ScrollView contentContainerStyle={styles.content}>
          {!bot && !detail.isPending && !detail.error && !readOnly ? (
            <BotForm
              key={`${serverId}:${workspaceId}`}
              serverId={serverId}
              workspaceId={workspaceId}
              cwd={cwd}
              interactive={interactive}
              onSaved={onChat}
            />
          ) : null}
          {detail.isPending ? <BotLoadingSpinner /> : null}
          {detail.error ? (
            <View style={styles.card}>
              <Text style={styles.error}>{detail.error.message}</Text>
              <Button
                variant="outline"
                onPress={() => {
                  void detail.refetch();
                }}
              >
                {t("bots.reload")}
              </Button>
            </View>
          ) : null}
          {bot && view === "settings" ? (
            <BotSettings
              key={bot.id}
              serverId={serverId}
              workspaceId={workspaceId}
              cwd={cwd}
              bot={bot}
              interactive={interactive}
              onSaved={onChat}
              onArchived={onChat}
            />
          ) : null}
        </ScrollView>
      )}
    </View>
  );
}

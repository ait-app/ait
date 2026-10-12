import React from "react";
import { ScrollView, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { BotLoadingSpinner } from "./loading";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { BotForm } from "./bot-form";
import { BotChat } from "./bot-chat";
import { BotSettings } from "./bot-settings";
import { botKeys, useBotDetail } from "./queries";
import { botStyles as styles } from "./styles";

export interface WorkspaceBotTabProps {
  serverId: string;
  workspaceId: string;
  cwd: string;
  botId?: string;
  view?: "list" | "create" | "settings";
  interactive: boolean;
  onBrowse: () => void;
  onCreate: () => void;
  onOpen: (botId: string) => void;
  onSettings: (botId: string) => void;
}

function BotList({
  serverId,
  workspaceId,
  interactive,
  onOpen,
}: Pick<WorkspaceBotTabProps, "serverId" | "workspaceId" | "interactive" | "onOpen">) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const bots = useQuery({
    queryKey: botKeys.list(serverId, workspaceId),
    enabled: interactive && Boolean(client),
    staleTime: 0,
    refetchInterval: 10_000,
    queryFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      return (await client.botRequest("bot.list.request", { workspaceId })).bots;
    },
  });
  return (
    <>
      {bots.isPending ? <BotLoadingSpinner /> : null}
      {bots.error ? (
        <View style={styles.card}>
          <Text style={styles.error}>{bots.error.message}</Text>
          <Button
            variant="outline"
            onPress={() => {
              void bots.refetch();
            }}
          >
            {t("bots.reload")}
          </Button>
        </View>
      ) : null}
      {bots.data?.length === 0 ? <Text style={styles.hint}>{t("bots.empty")}</Text> : null}
      {bots.data?.map((bot) => (
        <View key={bot.id} style={styles.card}>
          <View style={styles.row}>
            <Text style={[styles.title, styles.grow]}>{bot.name}</Text>
            <Button
              variant="outline"
              size="sm"
              onPress={() => onOpen(bot.id)}
              testID={`bot-open-${bot.id}`}
            >
              {t("bots.chat")}
            </Button>
          </View>
          <Text style={styles.hint}>
            {bot.execution.provider}
            {bot.execution.model ? ` · ${bot.execution.model}` : ""}
          </Text>
          {bot.archivedAt || !bot.enabled ? (
            <Text style={styles.hint}>{t(bot.archivedAt ? "bots.archived" : "bots.paused")}</Text>
          ) : null}
        </View>
      ))}
    </>
  );
}

export function WorkspaceBotTab(props: WorkspaceBotTabProps) {
  const {
    serverId,
    workspaceId,
    cwd,
    botId,
    view,
    interactive,
    onBrowse,
    onCreate,
    onOpen,
    onSettings,
  } = props;
  const { t } = useTranslation();
  const detail = useBotDetail(serverId, botId, interactive);
  // Persisted tabs may outlive workspace placement; never expose another workspace's Bot.
  const bot = detail.data?.workspaceId === workspaceId ? detail.data : undefined;
  return (
    <View style={styles.root} testID="workspace-bot-tab">
      <View style={styles.toolbar}>
        <Text numberOfLines={1} style={[styles.title, styles.grow]}>
          {bot?.name ?? t(view === "create" ? "bots.new" : "bots.browse")}
        </Text>
        {botId || view === "create" ? (
          <Button variant="ghost" size="sm" onPress={onBrowse}>
            {t("bots.browse")}
          </Button>
        ) : null}
        {bot ? (
          <>
            <Button
              variant={view === "settings" ? "ghost" : "outline"}
              size="sm"
              onPress={() => onOpen(bot.id)}
              testID="bot-show-chat"
            >
              {t("bots.chat")}
            </Button>
            <Button
              variant={view === "settings" ? "outline" : "ghost"}
              size="sm"
              onPress={() => onSettings(bot.id)}
              testID="bot-show-settings"
            >
              {t("bots.settings")}
            </Button>
          </>
        ) : (
          <Button
            size="sm"
            onPress={onCreate}
            disabled={!interactive || view === "create"}
            testID="bot-create-new"
          >
            {t("bots.new")}
          </Button>
        )}
      </View>
      {bot && view !== "settings" ? (
        <BotChat key={bot.id} serverId={serverId} bot={bot} interactive={interactive} />
      ) : (
        <ScrollView contentContainerStyle={styles.content}>
          {!botId && view === "create" ? (
            <BotForm
              key={`${serverId}:${workspaceId}`}
              serverId={serverId}
              workspaceId={workspaceId}
              cwd={cwd}
              interactive={interactive}
              onSaved={onOpen}
            />
          ) : null}
          {!botId && view !== "create" ? <BotList {...props} /> : null}
          {botId && detail.isPending ? <BotLoadingSpinner /> : null}
          {botId && (detail.error || (detail.data && !bot)) ? (
            <View style={styles.card}>
              <Text style={styles.error}>{detail.error?.message ?? t("bots.requestFailed")}</Text>
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
              onSaved={onOpen}
              onArchived={onBrowse}
            />
          ) : null}
        </ScrollView>
      )}
    </View>
  );
}

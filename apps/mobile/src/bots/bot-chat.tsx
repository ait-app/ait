import React, { useRef, useState } from "react";
import { FlatList, Text, View } from "react-native";
import { randomUUID } from "expo-crypto";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { Bot, BotTurn } from "@ait/protocol/bot/types";
import { Button } from "@/components/ui/button";
import { FormTextInput } from "@/components/ui/form-field";
import { BotLoadingSpinner } from "./loading";
import { MarkdownRenderer } from "@/components/markdown/renderer";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { confirmDialog } from "@/utils/confirm-dialog";
import { botKeys, isBotTurnPending, useBotHistory } from "./queries";
import { botStyles as styles } from "./styles";

function BotMessage({
  bot,
  turn,
  serverId,
  onRefresh,
}: {
  bot: Bot;
  turn: BotTurn;
  serverId: string;
  onRefresh: () => void;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const cancel = useMutation({
    mutationFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      await client.botRequest("bot.turn.cancel.request", { botId: bot.id, turnId: turn.id });
    },
    onSuccess: onRefresh,
  });
  return (
    <View style={styles.card} testID={`bot-turn-${turn.id}`}>
      <Text selectable style={styles.text}>
        {turn.text}
      </Text>
      <Text style={styles.hint}>{bot.name}</Text>
      {turn.reply ? <MarkdownRenderer text={turn.reply} /> : null}
      {turn.status !== "completed" ? (
        <Text style={styles.hint}>{t(`bots.status.${turn.status}`)}</Text>
      ) : null}
      {turn.error ? (
        <Text selectable style={styles.error}>
          {turn.error}
        </Text>
      ) : null}
      {isBotTurnPending(turn) ? (
        <Button
          size="sm"
          variant="outline"
          onPress={() => cancel.mutate()}
          disabled={cancel.isPending}
        >
          {t("bots.cancel")}
        </Button>
      ) : null}
      {cancel.error ? <Text style={styles.error}>{cancel.error.message}</Text> : null}
    </View>
  );
}

export function BotChat({
  serverId,
  bot,
  interactive,
}: {
  serverId: string;
  bot: Bot;
  interactive: boolean;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const queryClient = useQueryClient();
  const [text, setText] = useState("");
  const [inputRevision, setInputRevision] = useState(0);
  const attempt = useRef<{ text: string; conversationId: string; messageId: string } | null>(null);
  const list = useRef<FlatList<BotTurn>>(null);
  const atBottom = useRef(true);
  const conversation = useQuery({
    queryKey: botKeys.conversation(serverId, bot.id),
    enabled: interactive && Boolean(client),
    staleTime: 0,
    queryFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      if (bot.archivedAt) {
        const { conversations } = await client.botRequest("bot.conversation.list.request", {
          botId: bot.id,
        });
        return conversations.find((item) => item.channelId === bot.builtinChannelId) ?? null;
      }
      return (await client.botRequest("bot.conversation.open.request", { botId: bot.id }))
        .conversation;
    },
  });
  const history = useBotHistory(serverId, bot.id, conversation.data?.id, interactive);
  const refresh = () => {
    void history.refetch();
  };
  const send = useMutation({
    mutationFn: async () => {
      if (!client || !conversation.data) throw new Error(t("bots.disconnected"));
      if (new TextEncoder().encode(text).length > 16 * 1024) throw new Error(t("bots.tooLong"));
      // Keep the receipt identity after a timeout so retrying cannot duplicate input.
      if (
        attempt.current?.text !== text ||
        attempt.current?.conversationId !== conversation.data.id
      ) {
        attempt.current = { text, conversationId: conversation.data.id, messageId: randomUUID() };
      }
      return client.botRequest("bot.message.send.request", { botId: bot.id, ...attempt.current });
    },
    onSuccess: ({ turn }) => {
      queryClient.setQueryData<BotTurn[]>(
        botKeys.history(serverId, bot.id, turn.conversationId),
        (cached = []) =>
          [...cached.filter((item) => item.id !== turn.id), turn].sort(
            (left, right) => left.sequence - right.sequence,
          ),
      );
      setText("");
      setInputRevision((revision) => revision + 1);
      attempt.current = null;
      atBottom.current = true;
      refresh();
    },
  });
  const reset = useMutation({
    mutationFn: async () => {
      if (!client || !conversation.data) throw new Error(t("bots.disconnected"));
      const confirmed = await confirmDialog({
        title: t("bots.reset"),
        message: t("bots.resetConfirm"),
        confirmLabel: t("bots.reset"),
        cancelLabel: t("common.actions.cancel"),
      });
      if (!confirmed) return null;
      return (
        await client.botRequest("bot.conversation.reset.request", {
          botId: bot.id,
          conversationId: conversation.data.id,
        })
      ).conversation;
    },
    onSuccess: (updated) => {
      if (updated) {
        queryClient.setQueryData(botKeys.conversation(serverId, bot.id), updated);
        refresh();
      }
    },
  });
  const turns = history.data ?? [];
  const busy = turns.some(isBotTurnPending);
  const disabled =
    !interactive ||
    !bot.enabled ||
    Boolean(bot.archivedAt) ||
    !conversation.data ||
    reset.isPending;
  return (
    <View style={styles.root} testID="bot-chat">
      <View style={styles.toolbar}>
        <Text style={[styles.hint, styles.grow]}>{!bot.enabled ? t("bots.paused") : null}</Text>
        <Button
          variant="outline"
          size="sm"
          disabled={disabled || busy || send.isPending}
          onPress={() => reset.mutate()}
        >
          {t("bots.reset")}
        </Button>
      </View>
      {conversation.isPending && !conversation.data && !bot.archivedAt ? (
        <BotLoadingSpinner />
      ) : null}
      <FlatList
        ref={list}
        data={turns}
        keyExtractor={(turn) => turn.id}
        contentContainerStyle={styles.content}
        renderItem={({ item }) => (
          <BotMessage bot={bot} turn={item} serverId={serverId} onRefresh={refresh} />
        )}
        ListEmptyComponent={<Text style={styles.hint}>{t("bots.emptyChat")}</Text>}
        onScroll={({ nativeEvent }) => {
          atBottom.current =
            nativeEvent.contentSize.height -
              nativeEvent.layoutMeasurement.height -
              nativeEvent.contentOffset.y <
            80;
        }}
        scrollEventThrottle={100}
        onContentSizeChange={() => {
          if (atBottom.current) list.current?.scrollToEnd({ animated: false });
        }}
      />
      <View style={styles.composer}>
        {[conversation.error, history.error, send.error, reset.error]
          .filter(Boolean)
          .map((error, index) => (
            <Text key={index} style={styles.error} accessibilityRole="alert">
              {error?.message}
            </Text>
          ))}
        <FormTextInput
          initialValue=""
          resetKey={inputRevision}
          onChangeText={setText}
          multiline
          style={styles.messageInput}
          editable={!disabled && !send.isPending}
          accessibilityLabel={t("bots.message")}
          placeholder={t("bots.message")}
          testID="bot-message-input"
        />
        <Button
          onPress={() => send.mutate()}
          disabled={disabled || !text.trim() || send.isPending}
          testID="bot-message-send"
        >
          {t("bots.send")}
        </Button>
      </View>
    </View>
  );
}

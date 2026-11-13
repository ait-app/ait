import React, { useMemo } from "react";
import { View } from "react-native";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { useQuery } from "@tanstack/react-query";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import type { BotConversation } from "@ait/protocol/bot/types";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { KIND_LABELS } from "./channel-kinds";
import { BotSelect } from "./bot-select";
import { botKeys } from "./queries";
import { botChatStyles as chatStyles } from "./styles";

/** Whether `conversation` is the Bot's built-in Ait conversation, the only one Ait can send to. */
export function isBuiltinConversation(
  conversation: BotConversation,
  builtinChannelId: string,
): boolean {
  return conversation.channelKind === "builtin" || conversation.channelId === builtinChannelId;
}

/** The channel a conversation happens in, as people know it; older hosts report no kind. */
export function conversationChannel(conversation: BotConversation, t: TFunction): string {
  const kind = conversation.channelKind;
  return kind && kind in KIND_LABELS
    ? t(KIND_LABELS[kind as keyof typeof KIND_LABELS])
    : t("bots.conversationOtherChannel");
}

/**
 * A label a person recognizes a conversation by: Ait for the built-in one, otherwise its channel
 * with the chat, sender and thread ids, since hosts keep no display names of other platforms.
 */
export function conversationLabel(
  conversation: BotConversation,
  builtinChannelId: string,
  t: TFunction,
): string {
  if (isBuiltinConversation(conversation, builtinChannelId)) return t("bots.conversationOwner");
  const parts = [conversationChannel(conversation, t), conversation.peer];
  if (conversation.sender !== conversation.peer) parts.push(conversation.sender);
  const label = parts.join(" · ");
  return conversation.thread ? `${label} #${conversation.thread}` : label;
}

/** Every conversation of the Workspace Bot, most recently active first. */
export async function fetchBotConversations(
  client: Pick<DaemonClient, "botRequest">,
  workspaceId: string,
): Promise<BotConversation[]> {
  const { conversations } = await client.botRequest("bot.conversation.list.request", {
    workspaceId,
  });
  return [...conversations].sort((left, right) =>
    (right.lastActivityAt ?? "").localeCompare(left.lastActivityAt ?? ""),
  );
}

/** Every conversation of the Workspace Bot, shared with the chat's read of its own one. */
export function useBotConversations(
  serverId: string,
  workspaceId: string,
  builtinChannelId: string,
  enabled: boolean,
) {
  const client = useHostRuntimeClient(serverId);
  return useQuery({
    queryKey: botKeys.conversations(serverId, workspaceId, builtinChannelId),
    enabled: Boolean(enabled && client),
    refetchInterval: 10_000,
    queryFn: () => {
      if (!client) throw new Error("Bot unavailable");
      return fetchBotConversations(client, workspaceId);
    },
  });
}

/**
 * Chooses which conversation the Bot chat shows: the built-in one (`null`), which stays the
 * default, or another channel's conversation read-only. Hidden until one exists.
 */
export function BotConversationSwitch({
  conversations,
  builtinChannelId,
  value,
  onChange,
}: {
  conversations: BotConversation[];
  builtinChannelId: string;
  value: string | null;
  onChange: (conversationId: string | null) => void;
}) {
  const { t } = useTranslation();
  const external = useMemo(
    () => conversations.filter((item) => !isBuiltinConversation(item, builtinChannelId)),
    [builtinChannelId, conversations],
  );
  if (external.length === 0) return null;
  const options = [
    { id: "", label: t("bots.conversationOwner") },
    ...external.map((item) => ({
      id: item.id,
      label: conversationLabel(item, builtinChannelId, t),
    })),
  ];
  return (
    <View style={chatStyles.conversationSwitch}>
      <BotSelect
        label={t("bots.conversation")}
        value={value ?? ""}
        options={options}
        onChange={(id) => onChange(id || null)}
        testID="bot-conversation"
      />
    </View>
  );
}

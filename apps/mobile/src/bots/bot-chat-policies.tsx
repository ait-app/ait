import React from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { BotChat } from "@ait/protocol/bot/types";
import { SettingsSelect } from "@/components/settings";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { botKeys } from "./queries";

export function useBotChats(
  serverId: string,
  workspaceId: string,
  builtinChannelId: string,
  enabled: boolean,
) {
  const client = useHostRuntimeClient(serverId);
  const { t } = useTranslation();
  return useQuery({
    queryKey: botKeys.chats(serverId, workspaceId, builtinChannelId),
    enabled: enabled && Boolean(client),
    refetchInterval: 10_000,
    queryFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      return (await client.botRequest("bot.chat.list.request", { workspaceId })).chats;
    },
  });
}

type Reception = "off" | "mentions" | "all" | "direct";

/** One conversation row in its platform account's SettingsCard. */
export function BotChatPolicyRow({
  serverId,
  workspaceId,
  builtinChannelId,
  chat,
  platform,
  disabled,
}: {
  serverId: string;
  workspaceId: string;
  builtinChannelId: string;
  chat: BotChat;
  platform: string;
  disabled: boolean;
}) {
  const client = useHostRuntimeClient(serverId);
  const cache = useQueryClient();
  const { t } = useTranslation();
  const update = useMutation({
    mutationFn: async (mode: Reception) => {
      if (!client) throw new Error(t("bots.disconnected"));
      return client.botRequest("bot.chat.update.request", {
        workspaceId,
        channelId: chat.channelId,
        peer: chat.peer,
        enabled: mode !== "off",
        listenAllMessages: mode === "off" ? chat.listenAllMessages : mode === "all",
      });
    },
    onSuccess: ({ chat: updated }) => {
      cache.setQueryData<BotChat[]>(
        botKeys.chats(serverId, workspaceId, builtinChannelId),
        (chats) =>
          chats?.map((item) =>
            item.channelId === updated.channelId && item.peer === updated.peer ? updated : item,
          ),
      );
    },
  });
  const mode: Reception = !chat.enabled
    ? "off"
    : !chat.group
      ? "direct"
      : chat.listenAllMessages
        ? "all"
        : "mentions";
  const options: { value: Reception; label: string }[] = [
    { value: "off", label: t("bots.receptionOff") },
    ...(chat.group
      ? [
          { value: "mentions" as const, label: t("bots.receptionMentions") },
          { value: "all" as const, label: t("bots.receptionAll") },
        ]
      : [{ value: "direct" as const, label: t("bots.receptionDirect") }]),
  ];
  const hint =
    mode === "off"
      ? t("bots.chatDisabledHint")
      : mode === "all"
        ? t("bots.listenAllPermissions", { platform })
        : mode === "mentions"
          ? t("bots.mentionsOnly")
          : undefined;
  return (
    <SettingsSelect
      label={`${t(chat.group ? "bots.groupChat" : "bots.directChat")} · ${chat.peer}`}
      hint={hint}
      error={update.error?.message}
      value={mode}
      options={options}
      onValueChange={(next) => update.mutate(next)}
      disabled={disabled || !client || update.isPending}
      testID={`bot-chat-policy-${chat.channelId}-${chat.peer}`}
    />
  );
}

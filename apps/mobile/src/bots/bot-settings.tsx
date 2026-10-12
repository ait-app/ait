import React, { useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { Bot } from "@ait/protocol/bot/types";
import { Button } from "@/components/ui/button";
import { Field, FormTextInput } from "@/components/ui/form-field";
import { BotLoadingSpinner } from "./loading";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { confirmDialog } from "@/utils/confirm-dialog";
import { BotForm } from "./bot-form";
import { botKeys } from "./queries";
import { botStyles as styles } from "./styles";

function BotMemoryEditor({
  serverId,
  bot,
  memory,
  onReload,
}: {
  serverId: string;
  bot: Bot;
  memory: { text: string; revision: string };
  onReload: () => Promise<{ text: string; revision: string } | undefined>;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const queryClient = useQueryClient();
  const [draft, setDraft] = useState(memory);
  const [inputRevision, setInputRevision] = useState(0);
  const save = useMutation({
    mutationFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      if (new TextEncoder().encode(draft.text).length > 32 * 1024)
        throw new Error(t("bots.memoryTooLong"));
      try {
        const { revision } = await client.botRequest("bot.memory.update.request", {
          botId: bot.id,
          text: draft.text,
          expected: draft.revision,
        });
        return { text: draft.text, revision };
      } catch (cause) {
        if (cause instanceof Error && "code" in cause && cause.code === "bot_conflict")
          throw new Error(t("bots.memoryConflict"));
        throw cause;
      }
    },
    onSuccess: (updated) => {
      setDraft(updated);
      queryClient.setQueryData(botKeys.memory(serverId, bot.id), updated);
    },
  });
  const reload = useMutation({
    mutationFn: onReload,
    onSuccess: (updated) => {
      if (updated) {
        setDraft(updated);
        setInputRevision((revision) => revision + 1);
        save.reset();
      }
    },
  });
  return (
    <View style={styles.card}>
      <Field label={t("bots.memory")} hint={t("bots.memoryHint")}>
        <FormTextInput
          multiline
          initialValue={draft.text}
          resetKey={inputRevision}
          onChangeText={(text) => setDraft({ ...draft, text })}
          editable={!save.isPending && !reload.isPending && !bot.archivedAt}
          style={styles.multiline}
          accessibilityLabel={t("bots.memory")}
          testID="bot-memory"
        />
      </Field>
      {save.error ? <Text style={styles.error}>{save.error.message}</Text> : null}
      {reload.error ? <Text style={styles.error}>{reload.error.message}</Text> : null}
      <View style={styles.row}>
        <Button
          onPress={() => save.mutate()}
          disabled={
            save.isPending ||
            reload.isPending ||
            Boolean(bot.archivedAt) ||
            draft.text === memory.text
          }
          testID="bot-memory-save"
        >
          {t("bots.saveMemory")}
        </Button>
        <Button
          variant="outline"
          disabled={save.isPending || reload.isPending}
          onPress={() => reload.mutate()}
        >
          {t("bots.reload")}
        </Button>
      </View>
    </View>
  );
}

export function BotSettings({
  serverId,
  workspaceId,
  cwd,
  bot,
  interactive,
  onSaved,
  onArchived,
}: {
  serverId: string;
  workspaceId: string;
  cwd: string;
  bot: Bot;
  interactive: boolean;
  onSaved: (botId: string) => void;
  onArchived: () => void;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const queryClient = useQueryClient();
  const memory = useQuery({
    queryKey: botKeys.memory(serverId, bot.id),
    enabled: interactive && Boolean(client),
    queryFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      const { text, revision } = await client.botRequest("bot.memory.get.request", {
        botId: bot.id,
      });
      return { text, revision };
    },
  });
  const archive = useMutation({
    mutationFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      if (
        !(await confirmDialog({
          title: t("bots.archive"),
          message: t("bots.archiveConfirm"),
          confirmLabel: t("bots.archive"),
          cancelLabel: t("common.actions.cancel"),
          destructive: true,
        }))
      )
        return null;
      return (await client.botRequest("bot.archive.request", { botId: bot.id })).bot;
    },
    onSuccess: (archived) => {
      if (!archived) return;
      queryClient.setQueryData(botKeys.detail(serverId, bot.id), archived);
      void queryClient.invalidateQueries({ queryKey: botKeys.list(serverId, workspaceId) });
      onArchived();
    },
  });
  return (
    <>
      <BotForm
        serverId={serverId}
        workspaceId={workspaceId}
        cwd={cwd}
        bot={bot}
        interactive={interactive}
        onSaved={onSaved}
      />
      {memory.isPending ? <BotLoadingSpinner /> : null}
      {memory.error ? <Text style={styles.error}>{memory.error.message}</Text> : null}
      {memory.data ? (
        <BotMemoryEditor
          key={bot.id}
          serverId={serverId}
          bot={bot}
          memory={memory.data}
          onReload={async () => {
            const result = await memory.refetch({ throwOnError: true });
            return result.data;
          }}
        />
      ) : null}
      {archive.error ? <Text style={styles.error}>{archive.error.message}</Text> : null}
      {!bot.archivedAt ? (
        <Button
          variant="outline"
          disabled={!interactive || archive.isPending}
          onPress={() => archive.mutate()}
          testID="bot-archive"
        >
          {t("bots.archive")}
        </Button>
      ) : null}
    </>
  );
}

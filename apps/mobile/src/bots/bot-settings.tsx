import React, { useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { BotConversation, BotPermissions, BotView } from "@ait/protocol/bot/types";
import { Button } from "@/components/ui/button";
import { canManageBackgroundTasks } from "@/hosts/background-task-access";
import { Field, FormTextInput } from "@/components/ui/form-field";
import { BotChannels } from "./bot-channels";
import { BotLoadingSpinner } from "./loading";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { confirmDialog } from "@/utils/confirm-dialog";
import { BotPermissionFields, useBotPermissionsSupported } from "./bot-permissions";
import { BotExecutionFields, executionDraft, resolveExecution, useBotProviders } from "./bot-setup";
import { botKeys, isBotConflict } from "./queries";
import { closeBotSessionTab } from "./session-tab";
import { botStyles as styles } from "./styles";

function BotMemoryEditor({
  serverId,
  workspaceId,
  readOnly,
  active,
  memory,
  onReload,
}: {
  serverId: string;
  workspaceId: string;
  readOnly: boolean;
  /** The host accepts memory updates only while the Bot is active. */
  active: boolean;
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
          workspaceId,
          text: draft.text,
          expected: draft.revision,
        });
        return { text: draft.text, revision };
      } catch (cause) {
        if (!isBotConflict(cause)) throw cause;
        // A Bot paused elsewhere refuses updates with the same error as a stale revision.
        const { bot } = await client.botRequest("bot.get.request", { workspaceId });
        if (bot?.state.kind !== "active") {
          queryClient.setQueryData(botKeys.bot(serverId, workspaceId), bot);
          throw new Error(t("bots.resumeToEdit"));
        }
        throw new Error(t("bots.memoryConflict"));
      }
    },
    onSuccess: (updated) => {
      setDraft(updated);
      queryClient.setQueryData(botKeys.memory(serverId, workspaceId), updated);
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
          editable={!save.isPending && !reload.isPending && !readOnly}
          style={styles.multiline}
          accessibilityLabel={t("bots.memory")}
          testID="bot-memory"
        />
      </Field>
      {!readOnly && !active ? <Text style={styles.hint}>{t("bots.resumeToEdit")}</Text> : null}
      {save.error ? <Text style={styles.error}>{save.error.message}</Text> : null}
      {reload.error ? <Text style={styles.error}>{reload.error.message}</Text> : null}
      <View style={styles.row}>
        {!readOnly ? (
          <Button
            onPress={() => save.mutate()}
            disabled={save.isPending || reload.isPending || !active || draft.text === memory.text}
            testID="bot-memory-save"
          >
            {t("bots.saveMemory")}
          </Button>
        ) : null}
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

function BotModelSection({
  serverId,
  workspaceId,
  cwd,
  bot,
  interactive,
  readOnly,
  showMode,
}: {
  serverId: string;
  workspaceId: string;
  cwd: string;
  bot: BotView;
  interactive: boolean;
  readOnly: boolean;
  /** Hosts without Bot permissions still honor the Bot's own mode. */
  showMode: boolean;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const queryClient = useQueryClient();
  const [draft, setDraft] = useState(() => executionDraft(bot.execution));
  const { providers, error } = useBotProviders(serverId, cwd, interactive);
  const execution = resolveExecution(draft, providers);
  const save = useMutation({
    mutationFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      return (await client.botRequest("bot.update.request", { workspaceId, execution })).bot;
    },
    onSuccess: (updated) => {
      queryClient.setQueryData(botKeys.bot(serverId, workspaceId), updated);
    },
  });
  const disabled = readOnly || save.isPending;
  return (
    <View style={styles.card}>
      <BotExecutionFields
        providers={providers}
        error={error}
        value={draft}
        onChange={setDraft}
        disabled={disabled}
        showMode={showMode}
      />
      <Text style={styles.hint}>{t("bots.configurationHint")}</Text>
      {save.error ? <Text style={styles.error}>{save.error.message}</Text> : null}
      {!readOnly ? (
        <Button
          onPress={() => save.mutate()}
          disabled={disabled || !execution.provider}
          testID="bot-settings-save"
        >
          {t("bots.save")}
        </Button>
      ) : null}
    </View>
  );
}

function BotPermissionsSection({
  serverId,
  workspaceId,
  cwd,
  permissions,
  interactive,
  readOnly,
  saved,
  onSaved,
}: {
  serverId: string;
  workspaceId: string;
  cwd: string;
  permissions: BotPermissions;
  interactive: boolean;
  readOnly: boolean;
  /** Whether this screen saved the current permissions; kept by the parent across re-keys. */
  saved: boolean;
  onSaved: (saved: boolean) => void;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const queryClient = useQueryClient();
  const [draft, setDraft] = useState(permissions);
  const { providers } = useBotProviders(serverId, cwd, interactive);
  const save = useMutation({
    mutationFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      return await client.botRequest("bot.permissions.update.request", {
        workspaceId,
        permissions: draft,
      });
    },
    onSuccess: ({ bot }) => {
      onSaved(true);
      queryClient.setQueryData(botKeys.bot(serverId, workspaceId), bot);
    },
  });
  const disabled = readOnly || save.isPending;
  return (
    <View style={styles.card} testID="bot-permissions-section">
      <Text style={styles.title}>{t("bots.permissions")}</Text>
      <BotPermissionFields
        providers={providers}
        value={draft}
        onChange={(next) => {
          save.reset();
          onSaved(false);
          setDraft(next);
        }}
        disabled={disabled}
      />
      {save.error ? <Text style={styles.error}>{save.error.message}</Text> : null}
      {saved ? <Text style={styles.hint}>{t("bots.permissionsHint")}</Text> : null}
      {!readOnly ? (
        <Button
          onPress={() => save.mutate()}
          disabled={disabled || !interactive}
          testID="bot-permissions-save"
        >
          {t("bots.save")}
        </Button>
      ) : null}
    </View>
  );
}

export function BotSettings({
  serverId,
  workspaceId,
  cwd,
  bot,
  interactive,
  onRemoved,
}: {
  serverId: string;
  workspaceId: string;
  cwd: string;
  bot: BotView;
  interactive: boolean;
  onRemoved: () => void;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const queryClient = useQueryClient();
  const readOnly = !canManageBackgroundTasks(serverId);
  const supportsPermissions = useBotPermissionsSupported(serverId);
  const [permissionsSaved, setPermissionsSaved] = useState(false);
  const memory = useQuery({
    queryKey: botKeys.memory(serverId, workspaceId),
    enabled: interactive && Boolean(client),
    queryFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      const { text, revision } = await client.botRequest("bot.memory.get.request", {
        workspaceId,
      });
      return { text, revision };
    },
  });
  const toggle = useMutation({
    mutationFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      const method = bot.state.kind === "active" ? "bot.pause.request" : "bot.resume.request";
      return (await client.botRequest(method, { workspaceId })).bot;
    },
    onSuccess: (updated) => {
      queryClient.setQueryData(botKeys.bot(serverId, workspaceId), updated);
    },
  });
  const remove = useMutation({
    mutationFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      if (
        !(await confirmDialog({
          title: t("bots.remove"),
          message: t("bots.removeConfirm"),
          confirmLabel: t("bots.remove"),
          cancelLabel: t("common.actions.cancel"),
          destructive: true,
        }))
      )
        return false;
      // Read first: removal retires the sessions these conversations publish.
      const { conversations } = await client
        .botRequest("bot.conversation.list.request", { workspaceId })
        .catch(() => ({ conversations: [] as BotConversation[] }));
      await client.botRequest("bot.remove.request", { workspaceId });
      return conversations;
    },
    onSuccess: (removed) => {
      if (!removed) return;
      removed.forEach(({ sessionAgentId }) => {
        if (sessionAgentId) closeBotSessionTab(serverId, workspaceId, sessionAgentId);
      });
      queryClient.setQueryData(botKeys.bot(serverId, workspaceId), null);
      onRemoved();
    },
  });
  const busy = toggle.isPending || remove.isPending;
  return (
    <>
      <BotModelSection
        key={JSON.stringify(bot.execution)}
        serverId={serverId}
        workspaceId={workspaceId}
        cwd={cwd}
        bot={bot}
        interactive={interactive}
        readOnly={readOnly}
        showMode={!supportsPermissions}
      />
      {supportsPermissions && bot.permissions ? (
        <BotPermissionsSection
          // A change saved elsewhere replaces the draft.
          key={JSON.stringify(bot.permissions)}
          serverId={serverId}
          workspaceId={workspaceId}
          cwd={cwd}
          permissions={bot.permissions}
          interactive={interactive}
          readOnly={readOnly}
          saved={permissionsSaved}
          onSaved={setPermissionsSaved}
        />
      ) : null}
      {!readOnly && bot.state.kind !== "retired" ? (
        <View style={styles.card}>
          {toggle.error ? <Text style={styles.error}>{toggle.error.message}</Text> : null}
          <Button
            variant="outline"
            disabled={!interactive || busy}
            onPress={() => toggle.mutate()}
            testID="bot-toggle-state"
          >
            {bot.state.kind === "active" ? t("bots.pause") : t("bots.resume")}
          </Button>
        </View>
      ) : null}
      {memory.isPending && interactive ? <BotLoadingSpinner /> : null}
      {memory.error ? <Text style={styles.error}>{memory.error.message}</Text> : null}
      {memory.data ? (
        <BotMemoryEditor
          key={workspaceId}
          serverId={serverId}
          workspaceId={workspaceId}
          readOnly={readOnly}
          active={bot.state.kind === "active"}
          memory={memory.data}
          onReload={async () => {
            const result = await memory.refetch({ throwOnError: true });
            return result.data;
          }}
        />
      ) : null}
      <BotChannels
        serverId={serverId}
        workspaceId={workspaceId}
        builtinChannelId={bot.builtinChannelId}
        interactive={interactive}
        readOnly={readOnly}
        active={bot.state.kind === "active"}
      />
      {remove.error ? <Text style={styles.error}>{remove.error.message}</Text> : null}
      {!readOnly ? (
        <Button
          variant="outline"
          disabled={!interactive || busy}
          onPress={() => remove.mutate()}
          testID="bot-remove"
        >
          {t("bots.remove")}
        </Button>
      ) : null}
    </>
  );
}

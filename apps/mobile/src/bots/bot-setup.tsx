import React, { useState } from "react";
import { Pressable, Text, View } from "react-native";
import { Check } from "lucide-react-native";
import { withUnistyles } from "react-native-unistyles";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { ProviderSnapshotEntry } from "@ait/protocol/agent-types";
import type { BotExecution, BotPermissions, BotView, BotWakeScope } from "@ait/protocol/bot/types";
import { Button } from "@/components/ui/button";
import { useProvidersSnapshot } from "@/hooks/use-providers-snapshot";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import type { Theme } from "@/styles/theme";
import { BotWakeScopeField, useBotNotificationsSupported } from "./bot-notifications";
import { BotPermissionFields, useBotPermissionsSupported } from "./bot-permissions";
import { BotSelect } from "./bot-select";
import { botSessionNotes, defaultBotPermissions, type BotSessionNote } from "./permissions";
import { botKeys, isBotConflict } from "./queries";
import { botStyles as styles } from "./styles";

const ThemedCheck = withUnistyles(Check);
const checkedIconMapping = (theme: Theme) => ({ color: theme.colors.accentForeground });

/** Editable execution choice; an empty string stands for the provider default. */
export interface BotExecutionDraft {
  provider: string;
  model: string;
  modeId: string;
  thinking: string;
}

export function executionDraft(execution?: BotExecution): BotExecutionDraft {
  return {
    provider: execution?.provider ?? "",
    model: execution?.model ?? "",
    modeId: execution?.modeId ?? "",
    thinking: execution?.thinking ?? "",
  };
}

/** Ready providers the Bot can run on, from the host's provider snapshot. */
export function useBotProviders(serverId: string, cwd: string, enabled: boolean) {
  const snapshot = useProvidersSnapshot(serverId, { cwd, enabled });
  const providers = (snapshot.entries ?? []).filter(
    (entry) => entry.enabled && entry.status === "ready",
  );
  return { providers, error: snapshot.error };
}

const NOTE_KEYS: Record<BotSessionNote, string> = {
  instructionsInInput: "bots.noteInstructionsInInput",
  noReadOnlyMode: "bots.noteNoReadOnlyMode",
  toolsNeedApproval: "bots.noteToolsNeedApproval",
  noAitTools: "bots.noteNoAitTools",
};

/** Provider, model, optional permission mode and reasoning selectors shared by setup and settings. */
export function BotExecutionFields({
  providers,
  error,
  value,
  onChange,
  disabled,
  showMode = true,
}: {
  /**
   * Hosts with Bot permissions run the Bot in the strictest mode its provider offers, so Bot
   * screens hide the picker there; older hosts still honor the chosen mode.
   */
  showMode?: boolean;
  providers: ProviderSnapshotEntry[];
  error: string | null;
  value: BotExecutionDraft;
  onChange: (value: BotExecutionDraft) => void;
  disabled: boolean;
}) {
  const { t } = useTranslation();
  const selectedProvider = value.provider || providers[0]?.provider || "";
  const entry = providers.find((candidate) => candidate.provider === selectedProvider);
  const defaultOption = { id: "", label: t("bots.defaultOption") };
  const notes = botSessionNotes(entry?.botSession);
  const models = (entry?.models ?? []).filter((item) => item.isSelectable !== false);
  const selectedModel =
    models.find((item) => item.id === value.model) ?? models.find((item) => item.isDefault);
  return (
    <>
      <BotSelect
        label={t("bots.provider")}
        value={selectedProvider}
        options={providers.map((item) => ({
          id: item.provider,
          label: item.label || item.provider,
        }))}
        onChange={(provider) => onChange({ provider, model: "", modeId: "", thinking: "" })}
        disabled={disabled}
        testID="bot-provider"
      />
      <BotSelect
        label={t("bots.model")}
        value={value.model}
        options={[defaultOption, ...models]}
        onChange={(model) => onChange({ ...value, model, thinking: "" })}
        disabled={disabled}
        testID="bot-model"
      />
      {showMode ? (
        <BotSelect
          label={t("bots.mode")}
          value={value.modeId}
          options={[defaultOption, ...(entry?.modes ?? [])]}
          onChange={(modeId) => onChange({ ...value, modeId })}
          disabled={disabled}
          testID="bot-mode"
        />
      ) : (
        <Text style={styles.hint}>{t("bots.readOnlyHint")}</Text>
      )}
      {notes.map((note) => (
        <Text key={note} style={styles.hint} testID={`bot-note-${note}`}>
          {t(NOTE_KEYS[note])}
        </Text>
      ))}
      <BotSelect
        label={t("bots.thinking")}
        value={value.thinking}
        options={[defaultOption, ...(selectedModel?.thinkingOptions ?? [])]}
        onChange={(thinking) => onChange({ ...value, thinking })}
        disabled={disabled}
        testID="bot-thinking"
      />
      {error ? <Text style={styles.error}>{error}</Text> : null}
    </>
  );
}

/** Resolve a draft into the wire shape, falling back to the first ready provider. */
export function resolveExecution(
  draft: BotExecutionDraft,
  providers: ProviderSnapshotEntry[],
): BotExecution {
  return {
    provider: draft.provider || providers[0]?.provider || "",
    model: draft.model || null,
    modeId: draft.modeId || null,
    thinking: draft.thinking || null,
  };
}

export function BotSetup({
  serverId,
  workspaceId,
  cwd,
  interactive,
  onConfigured,
}: {
  serverId: string;
  workspaceId: string;
  cwd: string;
  interactive: boolean;
  onConfigured: (bot: BotView) => void;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const queryClient = useQueryClient();
  const [draft, setDraft] = useState(() => executionDraft());
  const [clearMemory, setClearMemory] = useState(false);
  const [permissionsDraft, setPermissionsDraft] = useState<BotPermissions | null>(null);
  const [wakeScope, setWakeScope] = useState<BotWakeScope>("own");
  const supportsPermissions = useBotPermissionsSupported(serverId);
  const supportsNotifications = useBotNotificationsSupported(serverId);
  const { providers, error } = useBotProviders(serverId, cwd, interactive);
  const execution = resolveExecution(draft, providers);
  // Providers load after mount, so the default profile is derived until the user edits it.
  const permissions = permissionsDraft ?? defaultBotPermissions(providers);
  const mutation = useMutation({
    mutationFn: async (clearingMemory: boolean) => {
      if (!client) throw new Error(t("bots.disconnected"));
      const { bot } = await client.botRequest("bot.setup.request", {
        workspaceId,
        execution,
        ...(supportsPermissions ? { permissions } : {}),
        ...(supportsNotifications ? { wakeScope } : {}),
        ...(clearingMemory ? { clearMemory: true } : {}),
      });
      return bot;
    },
    onSuccess: (bot, clearedMemory) => {
      // Conversation and channel caches are keyed by builtin channel; memory outlives a Bot.
      if (clearedMemory)
        queryClient.removeQueries({ queryKey: botKeys.memory(serverId, workspaceId) });
      queryClient.setQueryData(botKeys.bot(serverId, workspaceId), bot);
      onConfigured(bot);
    },
    onError: (cause) => {
      // Another device configured a Bot first; show it instead of the wizard.
      if (isBotConflict(cause))
        void queryClient.invalidateQueries({ queryKey: botKeys.bot(serverId, workspaceId) });
    },
  });
  const disabled = mutation.isPending || !interactive;
  return (
    <View style={styles.card} testID="bot-setup">
      <Text style={styles.title}>{t("bots.setupTitle")}</Text>
      <Text style={styles.hint}>{t("bots.setupHint")}</Text>
      <BotExecutionFields
        providers={providers}
        error={error}
        value={draft}
        onChange={setDraft}
        disabled={disabled}
        showMode={!supportsPermissions}
      />
      {supportsPermissions ? (
        <BotPermissionFields
          providers={providers}
          value={permissions}
          onChange={setPermissionsDraft}
          disabled={disabled}
        />
      ) : null}
      {supportsNotifications ? (
        <BotWakeScopeField value={wakeScope} onChange={setWakeScope} disabled={disabled} />
      ) : null}
      <Pressable
        style={[styles.row, disabled ? styles.disabled : null]}
        onPress={() => setClearMemory((checked) => !checked)}
        disabled={disabled}
        accessibilityRole="checkbox"
        accessibilityLabel={t("bots.clearMemory")}
        accessibilityState={{ checked: clearMemory, disabled }}
        aria-checked={clearMemory}
        testID="bot-clear-memory"
      >
        <View style={[styles.checkbox, clearMemory ? styles.checkboxChecked : null]}>
          {clearMemory ? <ThemedCheck size={14} uniProps={checkedIconMapping} /> : null}
        </View>
        <Text style={styles.text}>{t("bots.clearMemory")}</Text>
      </Pressable>
      {mutation.error ? (
        <Text style={styles.error} accessibilityRole="alert">
          {mutation.error.message}
        </Text>
      ) : null}
      <Button
        onPress={() => mutation.mutate(clearMemory)}
        disabled={disabled || !execution.provider}
        testID="bot-setup-submit"
      >
        {t("bots.setup")}
      </Button>
    </View>
  );
}

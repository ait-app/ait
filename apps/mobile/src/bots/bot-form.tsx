import React, { useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { randomUUID } from "expo-crypto";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { Bot } from "@ait/protocol/bot/types";
import { Button } from "@/components/ui/button";
import { Field, FormTextInput } from "@/components/ui/form-field";
import { SelectField } from "@/components/ui/select-field";
import { Switch } from "@/components/ui/switch";
import { useProvidersSnapshot } from "@/hooks/use-providers-snapshot";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { botKeys } from "./queries";
import { botStyles as styles } from "./styles";

function BotSelect({
  label,
  value,
  options,
  onChange,
  disabled = false,
  testID,
}: {
  label: string;
  value: string;
  options: { id: string; label: string }[];
  onChange: (value: string) => void;
  disabled?: boolean;
  testID: string;
}) {
  const { t } = useTranslation();
  return (
    <SelectField
      label={label}
      value={value}
      selectedDisplay={{
        label:
          options.find((option) => option.id === value)?.label || value || t("bots.defaultOption"),
      }}
      options={options.map((option) => ({ ...option, value: option.id }))}
      onChange={onChange}
      disabled={disabled}
      searchable
      placeholder={t("bots.defaultOption")}
      emptyText={t("bots.defaultOption")}
      testID={testID}
      triggerTestID={`${testID}-trigger`}
    />
  );
}

export function BotForm({
  serverId,
  workspaceId,
  cwd,
  bot,
  interactive,
  onSaved,
}: {
  serverId: string;
  workspaceId: string;
  cwd: string;
  bot?: Bot;
  interactive: boolean;
  onSaved: (botId: string) => void;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const queryClient = useQueryClient();
  const [botId] = useState(() => bot?.id ?? randomUUID());
  const [name, setName] = useState(bot?.name ?? "");
  const [instructions, setInstructions] = useState(bot?.instructions ?? "");
  const [provider, setProvider] = useState(bot?.execution.provider ?? "");
  const [model, setModel] = useState(bot?.execution.model ?? "");
  const [modeId, setModeId] = useState(bot?.execution.modeId ?? "");
  const [thinking, setThinking] = useState(bot?.execution.thinking ?? "");
  const [enabled, setEnabled] = useState(bot?.enabled ?? true);
  const snapshot = useProvidersSnapshot(serverId, { cwd, enabled: interactive });
  const providers = (snapshot.entries ?? []).filter(
    (entry) => entry.enabled && entry.status === "ready",
  );
  const selectedProvider = provider || providers[0]?.provider || "";
  const entry = providers.find((candidate) => candidate.provider === selectedProvider);
  const defaultOption = { id: "", label: t("bots.defaultOption") };
  const models = (entry?.models ?? []).filter((item) => item.isSelectable !== false);
  const selectedModel =
    models.find((item) => item.id === model) ?? models.find((item) => item.isDefault);
  const mutation = useMutation({
    mutationFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      const execution = {
        provider: selectedProvider,
        model: model || null,
        modeId: modeId || null,
        thinking: thinking || null,
      };
      if (bot)
        return (
          await client.botRequest("bot.update.request", {
            botId,
            name: name.trim(),
            instructions,
            execution,
            enabled,
          })
        ).bot;
      return (
        await client.botRequest("bot.create.request", {
          botId,
          workspaceId,
          name: name.trim(),
          instructions,
          execution,
        })
      ).bot;
    },
    onSuccess: (saved) => {
      queryClient.setQueryData(botKeys.detail(serverId, botId), saved);
      void queryClient.invalidateQueries({ queryKey: botKeys.list(serverId, workspaceId) });
      onSaved(botId);
    },
  });
  const disabled = mutation.isPending || Boolean(bot?.archivedAt) || !interactive;
  return (
    <View style={styles.card} testID="bot-form">
      <Field label={t("bots.name")}>
        <FormTextInput
          initialValue={name}
          onChangeText={setName}
          editable={!disabled}
          accessibilityLabel={t("bots.name")}
          testID="bot-name"
        />
      </Field>
      <Field label={t("bots.instructions")} hint={t("bots.instructionsHint")}>
        <FormTextInput
          initialValue={instructions}
          onChangeText={setInstructions}
          editable={!disabled}
          multiline
          style={styles.multiline}
          accessibilityLabel={t("bots.instructions")}
          testID="bot-instructions"
        />
      </Field>
      <BotSelect
        label={t("bots.provider")}
        value={selectedProvider}
        options={providers.map((item) => ({
          id: item.provider,
          label: item.label || item.provider,
        }))}
        onChange={(value) => {
          setProvider(value);
          setModel("");
          setModeId("");
          setThinking("");
        }}
        disabled={disabled}
        testID="bot-provider"
      />
      <BotSelect
        label={t("bots.model")}
        value={model}
        options={[defaultOption, ...models]}
        onChange={(value) => {
          setModel(value);
          setThinking("");
        }}
        disabled={disabled}
        testID="bot-model"
      />
      <BotSelect
        label={t("bots.mode")}
        value={modeId}
        options={[defaultOption, ...(entry?.modes ?? [])]}
        onChange={setModeId}
        disabled={disabled}
        testID="bot-mode"
      />
      <BotSelect
        label={t("bots.thinking")}
        value={thinking}
        options={[defaultOption, ...(selectedModel?.thinkingOptions ?? [])]}
        onChange={setThinking}
        disabled={disabled}
        testID="bot-thinking"
      />
      {bot ? (
        <View style={styles.row}>
          <Text style={styles.text}>{t("bots.enabled")}</Text>
          <Switch
            value={enabled}
            onValueChange={setEnabled}
            disabled={disabled}
            accessibilityLabel={t("bots.enabled")}
            testID="bot-enabled"
          />
        </View>
      ) : null}
      {bot ? <Text style={styles.hint}>{t("bots.configurationHint")}</Text> : null}
      {snapshot.error ? <Text style={styles.error}>{snapshot.error}</Text> : null}
      {mutation.error ? (
        <Text style={styles.error} accessibilityRole="alert">
          {mutation.error.message}
        </Text>
      ) : null}
      <Button
        onPress={() => mutation.mutate()}
        disabled={disabled || !name.trim() || !selectedProvider}
        testID="bot-save"
      >
        {bot ? t("bots.save") : t("bots.create")}
      </Button>
    </View>
  );
}

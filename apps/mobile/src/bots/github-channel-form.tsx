import React, { useMemo, useState } from "react";
import { Text, View } from "react-native";
import { randomUUID } from "expo-crypto";
import { useMutation } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { BotChannel } from "@ait/protocol/bot/types";
import { Button } from "@/components/ui/button";
import { Field, FormTextInput } from "@/components/ui/form-field";
import { SelectField } from "@/components/ui/select-field";
import { Switch } from "@/components/ui/switch";
import { useProjects } from "@/hooks/use-projects";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import {
  buildGithubChannelConfig,
  createGithubChannelDraft,
  githubWebhookPath,
  type GithubChannelDraft,
  type GithubDraftError,
  type GithubRepositoryDraft,
} from "./github-channel-model";
import { botStyles as styles } from "./styles";

interface ProjectOption {
  id: string;
  value: string;
  label: string;
  description: string;
}

function validationMessage(t: ReturnType<typeof useTranslation>["t"], error: GithubDraftError) {
  switch (error) {
    case "appId":
      return t("bots.githubChannel.validation.appId");
    case "installationId":
      return t("bots.githubChannel.validation.installationId");
    case "mention":
      return t("bots.githubChannel.validation.mention");
    case "privateKeyRef":
      return t("bots.githubChannel.validation.privateKeyRef");
    case "webhookSecretRef":
      return t("bots.githubChannel.validation.webhookSecretRef");
    case "repositories":
      return t("bots.githubChannel.validation.repositories");
    case "repository":
      return t("bots.githubChannel.validation.repository");
    case "duplicateRepository":
      return t("bots.githubChannel.validation.duplicateRepository");
    case "project":
      return t("bots.githubChannel.validation.project");
    case "allowedSenders":
      return t("bots.githubChannel.validation.allowedSenders");
  }
}

function RepositoryEditor({
  repository,
  projects,
  disabled,
  removable,
  onChange,
  onRemove,
}: {
  repository: GithubRepositoryDraft;
  projects: ProjectOption[];
  disabled: boolean;
  removable: boolean;
  onChange: (repository: GithubRepositoryDraft) => void;
  onRemove: () => void;
}) {
  const { t } = useTranslation();
  const selected = projects.find((project) => project.value === repository.projectId);
  return (
    <View style={styles.subcard} testID={`github-repository-${repository.key}`}>
      <View style={styles.sectionHeader}>
        <Text style={styles.subtitle}>{t("bots.githubChannel.repositoryBinding")}</Text>
        {removable ? (
          <Button variant="ghost" size="sm" disabled={disabled} onPress={onRemove}>
            {t("bots.githubChannel.removeRepository")}
          </Button>
        ) : null}
      </View>
      <Field label={t("bots.githubChannel.repository")}>
        <FormTextInput
          initialValue={repository.fullName}
          onChangeText={(fullName) => onChange({ ...repository, fullName })}
          editable={!disabled}
          autoCapitalize="none"
          autoCorrect={false}
          placeholder="owner/repository"
          testID={`github-repository-name-${repository.key}`}
        />
      </Field>
      <SelectField
        label={t("bots.githubChannel.project")}
        value={repository.projectId || null}
        selectedDisplay={
          selected ? { label: selected.label, description: selected.description } : null
        }
        options={projects}
        onChange={(projectId) => onChange({ ...repository, projectId })}
        placeholder={t("bots.githubChannel.selectProject")}
        emptyText={t("bots.githubChannel.noProjects")}
        searchable
        disabled={disabled}
      />
      <Field
        label={t("bots.githubChannel.allowedSenders")}
        hint={t("bots.githubChannel.allowedSendersHint")}
      >
        <FormTextInput
          initialValue={repository.allowedSenders}
          onChangeText={(allowedSenders) => onChange({ ...repository, allowedSenders })}
          editable={!disabled}
          autoCapitalize="none"
          autoCorrect={false}
          placeholder="maintainer, release-manager"
        />
      </Field>
    </View>
  );
}

export function GithubChannelForm({
  serverId,
  workspaceId,
  channel,
  interactive,
  active,
  onDone,
}: {
  serverId: string;
  workspaceId: string;
  channel?: BotChannel;
  interactive: boolean;
  active: boolean;
  onDone: () => void;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const projectDirectory = useProjects({ enabled: interactive });
  const projects = useMemo<ProjectOption[]>(
    () =>
      projectDirectory.projects.flatMap((project) =>
        project.hosts
          .filter((host) => host.serverId === serverId)
          .map((host) => ({
            id: host.projectId,
            value: host.projectId,
            label: host.projectCustomName || host.projectName,
            description: host.repoRoot,
          })),
      ),
    [projectDirectory.projects, serverId],
  );
  const [draft, setDraft] = useState<GithubChannelDraft>(() =>
    createGithubChannelDraft({
      channelId: randomUUID(),
      channel,
      repositoryKey: randomUUID(),
      defaultProjectId: projects[0]?.value,
    }),
  );
  const [validationError, setValidationError] = useState<string | null>(null);
  const save = useMutation({
    mutationFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      const result = buildGithubChannelConfig(draft);
      if (result.error) {
        const message = validationMessage(t, result.error);
        setValidationError(message);
        throw new Error(message);
      }
      setValidationError(null);
      if (channel) {
        await client.botRequest("bot.channel.update.request", {
          workspaceId,
          channelId: channel.id,
          config: result.config,
          enabled: draft.enabled,
        });
      } else {
        await client.botRequest("bot.channel.create.request", {
          workspaceId,
          channelId: draft.channelId,
          config: result.config,
          enabled: draft.enabled,
        });
      }
    },
    onSuccess: onDone,
  });
  const disabled = save.isPending || !interactive || !active;
  const updateRepository = (key: string, repository: GithubRepositoryDraft) =>
    setDraft((current) => ({
      ...current,
      repositories: current.repositories.map((item) => (item.key === key ? repository : item)),
    }));
  const webhook = `https://<public-host>${githubWebhookPath(draft.channelId)}`;
  return (
    <View style={styles.card} testID="github-channel-form">
      <Text style={styles.subtitle}>
        {channel ? t("bots.githubChannel.edit") : t("bots.githubChannel.add")}
      </Text>
      <Text style={styles.hint}>{t("bots.githubChannel.hint")}</Text>
      <Field label={t("bots.githubChannel.appId")}>
        <FormTextInput
          initialValue={draft.appId}
          onChangeText={(appId) => setDraft((current) => ({ ...current, appId }))}
          editable={!disabled}
          keyboardType="number-pad"
          testID="github-app-id"
        />
      </Field>
      <Field label={t("bots.githubChannel.installationId")}>
        <FormTextInput
          initialValue={draft.installationId}
          onChangeText={(installationId) => setDraft((current) => ({ ...current, installationId }))}
          editable={!disabled}
          keyboardType="number-pad"
          testID="github-installation-id"
        />
      </Field>
      <Field label={t("bots.githubChannel.mention")} hint={t("bots.githubChannel.mentionHint")}>
        <FormTextInput
          initialValue={draft.mention}
          onChangeText={(mention) => setDraft((current) => ({ ...current, mention }))}
          editable={!disabled}
          autoCapitalize="none"
          autoCorrect={false}
          placeholder="ait-review"
          testID="github-mention"
        />
      </Field>
      <Field
        label={t("bots.githubChannel.privateKeyRef")}
        hint={t("bots.githubChannel.secretHint")}
      >
        <FormTextInput
          initialValue={draft.privateKeyRef}
          onChangeText={(privateKeyRef) => setDraft((current) => ({ ...current, privateKeyRef }))}
          editable={!disabled}
          autoCapitalize="none"
          autoCorrect={false}
          testID="github-private-key-ref"
        />
      </Field>
      <Field
        label={t("bots.githubChannel.webhookSecretRef")}
        hint={t("bots.githubChannel.secretHint")}
      >
        <FormTextInput
          initialValue={draft.webhookSecretRef}
          onChangeText={(webhookSecretRef) =>
            setDraft((current) => ({ ...current, webhookSecretRef }))
          }
          editable={!disabled}
          autoCapitalize="none"
          autoCorrect={false}
          testID="github-webhook-secret-ref"
        />
      </Field>
      <View style={styles.subcard}>
        <Text style={styles.subtitle}>{t("bots.githubChannel.webhookUrl")}</Text>
        <Text style={styles.codeText} selectable>
          {webhook}
        </Text>
        <Text style={styles.hint}>{t("bots.githubChannel.webhookHint")}</Text>
      </View>
      <View style={styles.sectionHeader}>
        <Text style={styles.subtitle}>{t("bots.githubChannel.repositories")}</Text>
        <Button
          variant="outline"
          size="sm"
          disabled={disabled || draft.repositories.length >= 100}
          onPress={() =>
            setDraft((current) => ({
              ...current,
              repositories: [
                ...current.repositories,
                {
                  key: randomUUID(),
                  fullName: "",
                  projectId: projects[0]?.value ?? "",
                  allowedSenders: "",
                },
              ],
            }))
          }
          testID="github-repository-add"
        >
          {t("bots.githubChannel.addRepository")}
        </Button>
      </View>
      {draft.repositories.map((repository) => (
        <RepositoryEditor
          key={repository.key}
          repository={repository}
          projects={projects}
          disabled={disabled}
          removable={draft.repositories.length > 1}
          onChange={(updated) => updateRepository(repository.key, updated)}
          onRemove={() =>
            setDraft((current) => ({
              ...current,
              repositories: current.repositories.filter((item) => item.key !== repository.key),
            }))
          }
        />
      ))}
      <View style={styles.subcard}>
        <Text style={styles.subtitle}>{t("bots.githubChannel.triggers")}</Text>
        <Text style={styles.text}>✓ {t("bots.githubChannel.triggerMention")}</Text>
        <Text style={styles.text}>✓ {t("bots.githubChannel.triggerReviewer")}</Text>
        <Text style={styles.text}>✓ {t("bots.githubChannel.triggerAssignee")}</Text>
      </View>
      <View style={styles.row}>
        <Text style={styles.text}>{t("bots.channelEnabled")}</Text>
        <Switch
          value={draft.enabled}
          onValueChange={(enabled) => setDraft((current) => ({ ...current, enabled }))}
          disabled={disabled}
          testID="github-channel-enabled"
        />
      </View>
      {validationError ? <Text style={styles.error}>{validationError}</Text> : null}
      {save.error && save.error.message !== validationError ? (
        <Text style={styles.error}>{save.error.message}</Text>
      ) : null}
      <View style={styles.row}>
        <Button disabled={disabled} onPress={() => save.mutate()} testID="github-channel-save">
          {t("bots.save")}
        </Button>
        <Button variant="outline" disabled={save.isPending} onPress={onDone}>
          {t("common.actions.cancel")}
        </Button>
      </View>
    </View>
  );
}

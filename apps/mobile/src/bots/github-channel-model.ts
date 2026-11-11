import type { BotChannel, BotChannelConfig } from "@ait/protocol/bot/types";

export interface GithubRepositoryDraft {
  key: string;
  fullName: string;
  projectId: string;
  allowedSenders: string;
}

export interface GithubChannelDraft {
  channelId: string;
  appId: string;
  installationId: string;
  mention: string;
  privateKeyRef: string;
  webhookSecretRef: string;
  enabled: boolean;
  repositories: GithubRepositoryDraft[];
}

export type GithubChannelConfig = Extract<BotChannelConfig, { kind: "github" }>;

export type GithubDraftError =
  | "appId"
  | "installationId"
  | "mention"
  | "privateKeyRef"
  | "webhookSecretRef"
  | "repositories"
  | "repository"
  | "project"
  | "allowedSenders"
  | "duplicateRepository";

export type GithubDraftResult =
  | { config: GithubChannelConfig; error: null }
  | { config: null; error: GithubDraftError };

const NUMERIC_ID = /^[1-9][0-9]{0,31}$/;
const LOGIN = /^[A-Za-z0-9-]{1,100}$/;
const REPOSITORY_PART = /^(?!\.{1,2}$)[A-Za-z0-9_.-]{1,100}$/;
const CREDENTIAL_REFERENCE = /^env:AIT_SERVER_CREDENTIAL_[A-Z0-9_]{1,96}$/;

function parseSenders(value: string): string[] {
  return value
    .split(/[\s,]+/)
    .map((sender) => sender.trim())
    .filter(Boolean);
}

export function createGithubChannelDraft(input: {
  channelId: string;
  channel?: BotChannel;
  repositoryKey: string;
  defaultProjectId?: string;
}): GithubChannelDraft {
  const config = input.channel?.config.kind === "github" ? input.channel.config : null;
  return {
    channelId: input.channel?.id ?? input.channelId,
    appId: config?.appId ?? "",
    installationId: config?.installationId ?? "",
    mention: config?.mention ?? "",
    privateKeyRef: config?.privateKeyRef ?? "env:AIT_SERVER_CREDENTIAL_GITHUB_APP_PRIVATE_KEY",
    webhookSecretRef: config?.webhookSecretRef ?? "env:AIT_SERVER_CREDENTIAL_GITHUB_WEBHOOK_SECRET",
    enabled: input.channel?.enabled ?? true,
    repositories: config
      ? config.repositories.map((repository, index) => ({
          key: `${input.repositoryKey}-${index}`,
          fullName: repository.fullName,
          projectId: repository.projectId,
          allowedSenders: repository.allowedSenders.join(", "),
        }))
      : [
          {
            key: input.repositoryKey,
            fullName: "",
            projectId: input.defaultProjectId ?? "",
            allowedSenders: "",
          },
        ],
  };
}

export function buildGithubChannelConfig(draft: GithubChannelDraft): GithubDraftResult {
  const appId = draft.appId.trim();
  if (!NUMERIC_ID.test(appId)) return { config: null, error: "appId" };
  const installationId = draft.installationId.trim();
  if (!NUMERIC_ID.test(installationId)) return { config: null, error: "installationId" };
  const mention = draft.mention
    .trim()
    .replace(/^@/, "")
    .replace(/\[bot\]$/i, "");
  if (!LOGIN.test(mention)) return { config: null, error: "mention" };
  const privateKeyRef = draft.privateKeyRef.trim();
  if (!CREDENTIAL_REFERENCE.test(privateKeyRef)) return { config: null, error: "privateKeyRef" };
  const webhookSecretRef = draft.webhookSecretRef.trim();
  if (!CREDENTIAL_REFERENCE.test(webhookSecretRef))
    return { config: null, error: "webhookSecretRef" };
  if (!draft.repositories.length || draft.repositories.length > 100)
    return { config: null, error: "repositories" };

  const names = new Set<string>();
  const repositories: GithubChannelConfig["repositories"] = [];
  for (const repository of draft.repositories) {
    const fullName = repository.fullName.trim();
    const parts = fullName.split("/");
    if (parts.length !== 2 || !parts.every((part) => REPOSITORY_PART.test(part)))
      return { config: null, error: "repository" };
    const normalizedName = fullName.toLowerCase();
    if (names.has(normalizedName)) return { config: null, error: "duplicateRepository" };
    names.add(normalizedName);
    const projectId = repository.projectId.trim();
    if (!projectId) return { config: null, error: "project" };
    const allowedSenders = parseSenders(repository.allowedSenders);
    if (
      !allowedSenders.length ||
      allowedSenders.length > 256 ||
      !allowedSenders.every((sender) => LOGIN.test(sender))
    )
      return { config: null, error: "allowedSenders" };
    repositories.push({ fullName, projectId, allowedSenders });
  }

  return {
    error: null,
    config: {
      kind: "github",
      appId,
      privateKeyRef,
      webhookSecretRef,
      installationId,
      mention,
      repositories,
    },
  };
}

export function githubWebhookPath(channelId: string): string {
  return `/v1/bots/github/${channelId}/webhook`;
}

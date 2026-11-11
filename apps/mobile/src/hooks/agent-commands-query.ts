import type { AgentProvider } from "@ait/protocol/agent-types";
import { normalizeWorkspacePath } from "@/utils/workspace-identity";

export const AGENT_COMMANDS_QUERY_ROOT = "agentCommands";

const commandClients = new WeakMap<object, number>();
let nextCommandClient = 0;

/** A replacement connection must not inherit an old client's in-flight ownership response. */
export function agentCommandClientKey(client: object | null | undefined): number | null {
  if (!client) return null;
  let key = commandClients.get(client);
  if (key === undefined) {
    key = ++nextCommandClient;
    commandClients.set(client, key);
  }
  return key;
}

export interface AgentCommandsDraftConfig {
  provider: AgentProvider;
  cwd: string;
  modeId?: string;
  model?: string;
  thinkingOptionId?: string;
  featureValues?: Record<string, unknown>;
}

export interface AgentCommandContext {
  provider: string;
  cwd: string;
  nativeSessionId: string | null;
}

export function normalizeAgentCommandsCwd(cwd: string): string {
  return normalizeWorkspacePath(cwd) ?? "";
}

export function agentCommandsQueryRoot(serverId: string) {
  return [AGENT_COMMANDS_QUERY_ROOT, serverId] as const;
}

export function sessionAgentCommandsQueryKey(input: {
  serverId: string;
  agentId: string;
  context?: AgentCommandContext;
}) {
  return [
    ...agentCommandsQueryRoot(input.serverId),
    "session",
    input.agentId,
    input.context,
  ] as const;
}

export function draftAgentCommandsQueryKey(input: {
  serverId: string;
  draftConfig: AgentCommandsDraftConfig;
}) {
  const { draftConfig } = input;
  return [
    ...agentCommandsQueryRoot(input.serverId),
    "draft",
    draftConfig.provider,
    "cwd",
    normalizeAgentCommandsCwd(draftConfig.cwd),
    "mode",
    draftConfig.modeId ?? null,
    "model",
    draftConfig.model ?? null,
    "thinking",
    draftConfig.thinkingOptionId ?? null,
    "features",
    draftConfig.featureValues ?? null,
  ] as const;
}

export function agentCommandsQueryKey(input: {
  serverId: string;
  agentId: string;
  draftConfig?: AgentCommandsDraftConfig;
  context?: AgentCommandContext;
  client?: object | null;
}) {
  const scope = input.draftConfig
    ? draftAgentCommandsQueryKey({ serverId: input.serverId, draftConfig: input.draftConfig })
    : sessionAgentCommandsQueryKey(input);
  return [...scope, agentCommandClientKey(input.client)] as const;
}

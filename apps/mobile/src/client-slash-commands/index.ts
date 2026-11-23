import type { Agent } from "@/stores/session-store";
import type { WorkspaceDraftTabSetup } from "@/workspace-tabs/model";

export type ClientSlashCommandKind = "archive-agent" | "replace-agent-with-draft";
export type ClientSlashCommandExecution = "immediate" | "insert";

export interface ClientSlashCommand {
  name: string;
  aliases: readonly string[];
  description: string;
  descriptionKey: "composer.clientCommands.archiveAgent" | "composer.clientCommands.freshDraft";
  argumentHint: string;
  kind: ClientSlashCommandKind;
  execution: ClientSlashCommandExecution;
}

export const CLIENT_SLASH_COMMANDS: readonly ClientSlashCommand[] = [
  {
    name: "exit",
    aliases: ["quit", "q"],
    description: "Archive the current agent",
    descriptionKey: "composer.clientCommands.archiveAgent",
    argumentHint: "",
    kind: "archive-agent",
    execution: "immediate",
  },
  {
    name: "clear",
    aliases: ["new"],
    description: "Archive this agent and start a fresh draft",
    descriptionKey: "composer.clientCommands.freshDraft",
    argumentHint: "",
    kind: "replace-agent-with-draft",
    execution: "immediate",
  },
];

export function selectedClientCommandName(
  command: ClientSlashCommand,
  query: string | undefined,
): string {
  return query && [command.name, ...command.aliases].includes(query) ? query : command.name;
}

const COMMAND_BY_NAME = new Map<string, ClientSlashCommand>();
for (const command of CLIENT_SLASH_COMMANDS) {
  COMMAND_BY_NAME.set(command.name, command);
  for (const alias of command.aliases) {
    COMMAND_BY_NAME.set(alias, command);
  }
}

export interface NativeCommandClaim {
  name: string;
  source?: string;
  aliases?: readonly string[];
  menuVisible?: boolean;
  menuAliases?: readonly string[];
}

/** Multiple native claimants for one name produce one insertable menu row. */
export function visibleNativeCommands<T extends NativeCommandClaim>(commands: readonly T[]): T[] {
  const visible = new Map<string, T>();
  for (const command of commands) {
    if (command.menuVisible === false) continue;
    const key = command.name.toLowerCase();
    const previous = visible.get(key);
    if (!previous) {
      visible.set(key, command);
      continue;
    }
    const representative =
      previous.source === "builtin" && command.source !== "builtin" ? command : previous;
    const aliases = new Set([
      ...(previous.menuAliases ?? previous.aliases ?? []),
      ...(command.menuAliases ?? command.aliases ?? []),
    ]);
    visible.set(key, { ...representative, menuAliases: [...aliases] });
  }
  return [...visible.values()];
}

/** Only an explicitly GUI-managed builtin yields its own name to a client shortcut. */
export function nativeCommandOwnsName(
  name: string,
  commands: readonly NativeCommandClaim[],
): boolean {
  const normalized = name.toLowerCase();
  return commands.some((entry) => {
    const aliasMatch = entry.aliases?.some(
      (alias) => alias.replace(/:+$/u, "").toLowerCase() === normalized,
    );
    if (!aliasMatch && entry.name.toLowerCase() !== normalized) return false;
    return (
      entry.source !== "builtin" ||
      entry.menuVisible !== false ||
      COMMAND_BY_NAME.get(entry.name.toLowerCase()) !== COMMAND_BY_NAME.get(normalized)
    );
  });
}

export type CommandOwnership =
  | { kind: "legacy" }
  | { kind: "unavailable" }
  | { kind: "complete"; commands: readonly NativeCommandClaim[] };

/** A builtin and a non-builtin claimant do not prove which native handler wins. */
export function commandOwnershipAmbiguous(text: string, ownership: CommandOwnership): boolean {
  const match = /^\s*\/([^\s/]+)\s*$/u.exec(text);
  if (!match || ownership.kind !== "complete") return false;
  const name = match[1].toLowerCase();
  const matching = ownership.commands.filter((entry) =>
    [entry.name, ...(entry.aliases ?? [])].some(
      (claim) => claim.replace(/:+$/u, "").toLowerCase() === name,
    ),
  );
  return (
    matching.some((entry) => entry.source === "builtin") &&
    matching.some((entry) => entry.source !== "builtin")
  );
}

export function resolveClientSlashCommand(input: {
  text: string;
  hasAttachments: boolean;
  ownership?: CommandOwnership;
}): ClientSlashCommand | null {
  if (input.hasAttachments) {
    return null;
  }

  const trimmed = input.text.trim();
  if (!trimmed.startsWith("/")) {
    return null;
  }

  const commandName = trimmed.slice(1);
  if (!commandName || /\s/.test(commandName)) {
    return null;
  }

  if (
    input.ownership?.kind === "unavailable" ||
    (input.ownership?.kind === "complete" &&
      nativeCommandOwnsName(commandName, input.ownership.commands))
  ) {
    return null;
  }

  return COMMAND_BY_NAME.get(commandName) ?? null;
}

export function buildDraftAgentSetup(agent: Agent): WorkspaceDraftTabSetup {
  const featureValues: Record<string, unknown> = {};
  for (const feature of agent.features ?? []) {
    featureValues[feature.id] = feature.value;
  }

  return {
    provider: agent.provider,
    cwd: agent.cwd,
    modeId: agent.currentModeId ?? agent.runtimeInfo?.modeId ?? null,
    model: agent.model ?? agent.runtimeInfo?.model ?? null,
    thinkingOptionId: agent.thinkingOptionId ?? agent.runtimeInfo?.thinkingOptionId ?? null,
    featureValues,
  };
}

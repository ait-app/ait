import type { AgentSlashCommand } from "@/hooks/use-agent-commands-query";

/** Return only declared metadata for an otherwise empty argument area; never edit draft text. */
export function commandArgumentHint(text: string, commands: AgentSlashCommand[]) {
  const match = /^\/([^\s/]+)[ \t]+$/.exec(text);
  if (!match) return null;
  const command = commands.find(
    (entry) => entry.name === match[1] || entry.aliases?.includes(match[1]!),
  );
  const hint = command?.argumentHint.trim();
  return hint ? { prefix: text, hint } : null;
}

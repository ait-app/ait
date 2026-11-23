import type { AgentSlashCommand } from "@/hooks/use-agent-commands-query";
import { commandOwnershipAmbiguous } from "@/client-slash-commands";

/** Hints are only shown before arguments are typed; they never become input text. */
export function commandArgumentHint(
  text: string,
  commands: readonly AgentSlashCommand[],
): string | null {
  const match = /^\/([^\s/]+)[ \t]+$/u.exec(text);
  if (!match) return null;
  const name = match[1];
  if (commandOwnershipAmbiguous(`/${name}`, { kind: "complete", commands })) return null;
  const matching = commands.filter(
    (command) =>
      command.menuVisible !== false &&
      (command.name === name || (command.menuAliases ?? command.aliases)?.includes(name)),
  );
  const hints = new Set(matching.map((command) => command.argumentHint.trim()).filter(Boolean));
  if (hints.size !== 1) return null;
  const hint = [...hints][0];
  return hint && !/[\r\n\u2028\u2029]/u.test(hint) ? hint : null;
}

import {
  applySlashCommandReplacement,
  findActiveSlashCommand,
} from "@/utils/agent-command-autocomplete";

/** Apply a menu command at the start, preserving the user's existing text as its arguments. */
export function featureCommandDraft(text: string, command: string): string {
  const separator = text.search(/\s/u);
  const active = findActiveSlashCommand({
    text,
    cursorIndex: separator < 0 ? text.length : separator,
  });
  if (active?.position === "start") {
    return applySlashCommandReplacement({
      text,
      command: active,
      commandName: command.trim().replace(/^\//u, ""),
    });
  }
  return `${command}${text}`;
}

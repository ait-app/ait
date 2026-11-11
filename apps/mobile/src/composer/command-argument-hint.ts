import type { AgentSlashCommand } from "@/hooks/use-agent-commands-query";

/**
 * The overlay is absolutely positioned over the editor's first line, so it may only describe
 * the single argument area the platform input shows. A line break starts a line with its own
 * alignment, so a multiline draft never gets the overlay.
 */
const LINE_BREAK = /[\r\n\u2028\u2029]/;

/** Native `split` ends the name at the first whitespace or colon and accepts both as separators. */
const ARGUMENT_AREA = /^\/([^\s/:]+)([\s:]+)$/;

/** Native strips leading colons, then whitespace: a colon after a space is already argument. */
const EMPTY_ARGUMENT = /^:*\s*$/;

function matchArgumentArea(text: string): { prefix: string; name: string } | null {
  if (LINE_BREAK.test(text)) return null;
  const match = ARGUMENT_AREA.exec(text);
  const name = match?.[1];
  const separator = match?.[2];
  if (!name || !separator || !EMPTY_ARGUMENT.test(separator)) return null;
  return { prefix: text, name };
}

/** True when the draft holds a command name and nothing after it but argument separators. */
export function isCommandArgumentArea(text: string): boolean {
  return matchArgumentArea(text) !== null;
}

/** Native aliases keep the trailing separator colon the catalog reports verbatim. */
function ownsName(command: AgentSlashCommand, name: string): boolean {
  return (
    command.name === name ||
    command.aliases?.some((alias) => alias.replace(/:+$/, "") === name) === true
  );
}

/** Return only declared metadata for an otherwise empty argument area; never edit draft text. */
export function commandArgumentHint(text: string, commands: AgentSlashCommand[]) {
  const area = matchArgumentArea(text);
  if (!area) return null;
  const command = commands.find((entry) => ownsName(entry, area.name));
  // Metadata is painted on one line; a hint with its own line break has no honest projection.
  const hint = command?.argumentHint.trim();
  return hint && !LINE_BREAK.test(hint) ? { prefix: area.prefix, hint } : null;
}

/**
 * The platform paints uncommitted composition candidates at the caret, which is exactly where
 * the overlay's text starts. Composition targets are tracked until commit so the hint yields
 * for the whole candidate, instead of flickering with each composition update or doubling it.
 */
const composingTargets = new Set<Node>();
const compositionListeners = new Set<() => void>();
let compositionAttached = false;

function handleCompositionEvent(event: Event): void {
  if (!(event.target instanceof Node)) return;
  const started = event.type === "compositionstart";
  if (started) composingTargets.add(event.target);
  else composingTargets.delete(event.target);
  for (const listener of compositionListeners) listener();
}

/** Snapshot for `useSyncExternalStore`; a detached editor can never strand an active candidate. */
export function getImeComposingSnapshot(): boolean {
  for (const target of composingTargets) if (!target.isConnected) composingTargets.delete(target);
  return composingTargets.size > 0;
}

/** Observe DOM composition only while an overlay subscribes; native input owns its own IME. */
export function subscribeToImeComposition(listener: () => void): () => void {
  compositionListeners.add(listener);
  if (!compositionAttached && typeof document !== "undefined") {
    document.addEventListener("compositionstart", handleCompositionEvent, true);
    document.addEventListener("compositionend", handleCompositionEvent, true);
    compositionAttached = true;
  }
  return () => {
    compositionListeners.delete(listener);
    if (compositionListeners.size > 0) return;
    if (compositionAttached) {
      document.removeEventListener("compositionstart", handleCompositionEvent, true);
      document.removeEventListener("compositionend", handleCompositionEvent, true);
      compositionAttached = false;
    }
    composingTargets.clear();
  };
}

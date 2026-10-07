import type { ComposerInputSnapshot } from "../input/input";

/** Insert literal paths at the current selection without replacing the rest of the draft. */
export function insertDirectoryPaths(
  input: ComposerInputSnapshot,
  paths: readonly string[],
): ComposerInputSnapshot {
  if (paths.length === 0) return input;
  const before = input.text.slice(0, input.selection.start);
  const after = input.text.slice(input.selection.end);
  const prefix = before && !/\s$/.test(before) ? "\n" : "";
  const inserted = `${prefix}${paths.join("\n")}\n`;
  const cursor = before.length + inserted.length;
  return { text: before + inserted + after, selection: { start: cursor, end: cursor } };
}

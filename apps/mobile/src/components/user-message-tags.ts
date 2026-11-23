/**
 * Splits user-message text into prose and the XML-style context blocks that harnesses inject
 * into a prompt, such as Claude Code's `<system-reminder>`, pasted-content wrappers, or IDE
 * selection context. Sessions imported from a native transcript keep these blocks in the
 * user's text; the bubble shows them as collapsed sections instead of raw markup.
 *
 * A block is recognized only when it can't be ordinary prose:
 * - the tag name has a `-` or `_` separator, so HTML such as `<div>` stays text;
 * - the opening tag starts a line and the matching closing tag ends one, so a sentence that
 *   mentions `<system-reminder>` inline stays text;
 * - an unclosed tag stays text.
 */
export type UserMessageSegment =
  | { kind: "text"; text: string }
  | { kind: "tag"; name: string; body: string };

const OPENING_TAG = /^[ \t]*<([A-Za-z][A-Za-z0-9]*(?:[-_][A-Za-z0-9]+)+)(?:\s[^<>]*)?>/gm;
const LINE_END = /^[ \t]*(?:\r?\n|$)/;

export function splitUserMessageTags(message: string): UserMessageSegment[] {
  if (!message.includes("</")) {
    return [{ kind: "text", text: message }];
  }
  const segments: UserMessageSegment[] = [];
  let cursor = 0;
  OPENING_TAG.lastIndex = 0;
  for (let open = OPENING_TAG.exec(message); open; open = OPENING_TAG.exec(message)) {
    const name = open[1];
    const bodyStart = open.index + open[0].length;
    const close = findClosingTag(message, name, bodyStart);
    if (!close) {
      continue;
    }
    pushText(segments, message.slice(cursor, open.index));
    segments.push({ kind: "tag", name, body: message.slice(bodyStart, close.start).trim() });
    cursor = close.end;
    OPENING_TAG.lastIndex = close.end;
  }
  if (cursor === 0) {
    return [{ kind: "text", text: message }];
  }
  pushText(segments, message.slice(cursor));
  return segments;
}

/** Finds the `</name>` that balances an opening tag and ends its line. */
function findClosingTag(
  message: string,
  name: string,
  from: number,
): { start: number; end: number } | null {
  // Tag names only hold letters, digits, `-` and `_`, so they need no escaping.
  const tags = new RegExp(`<(/?)${name}(?=[\\s>])[^<>]*>`, "g");
  tags.lastIndex = from;
  let depth = 1;
  for (let tag = tags.exec(message); tag; tag = tags.exec(message)) {
    depth += tag[1] ? -1 : 1;
    if (depth > 0) {
      continue;
    }
    const end = tag.index + tag[0].length;
    const lineEnd = LINE_END.exec(message.slice(end));
    return lineEnd ? { start: tag.index, end: end + lineEnd[0].length } : null;
  }
  return null;
}

function pushText(segments: UserMessageSegment[], text: string) {
  const trimmed = text.replace(/^(?:[ \t]*\r?\n)+/, "").replace(/(?:\r?\n[ \t]*)+$/, "");
  if (trimmed.trim()) {
    segments.push({ kind: "text", text: trimmed });
  }
}

/** The prose of a user message without its tag blocks, for one-line titles and previews. */
export function userMessageProse(message: string): string {
  return splitUserMessageTags(message)
    .flatMap((segment) => (segment.kind === "text" ? [segment.text] : []))
    .join("\n")
    .trim();
}

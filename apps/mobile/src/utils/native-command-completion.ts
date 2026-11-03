import type { CommandCompletionItem } from "@ait/protocol/messages";

function isBoundary(text: string, offset: number): boolean {
  if (!Number.isInteger(offset) || offset < 0 || offset > text.length) return false;
  const before = text.charCodeAt(offset - 1);
  const after = text.charCodeAt(offset);
  return !(before >= 0xd800 && before <= 0xdbff && after >= 0xdc00 && after <= 0xdfff);
}

/** Apply the native edit exactly, including its quoting, suffix and explicit caret. */
export function applyNativeCommandCompletion(text: string, item: CommandCompletionItem) {
  const { start, end } = item.replaceRange;
  if (start > end || !isBoundary(text, start) || !isBoundary(text, end)) return null;
  const nextText = text.slice(0, start) + item.insertText + text.slice(end);
  const cursor = item.cursor ?? start + item.insertText.length;
  if (!isBoundary(nextText, cursor)) return null;
  return { text: nextText, cursor };
}

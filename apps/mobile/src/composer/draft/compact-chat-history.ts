import type { AgentAttachment } from "@ait/protocol/messages";
import type { DaemonClient } from "@ait/client/internal/daemon-client";

const COMPACT_AFTER_BYTES = 48 * 1024;
const RECENT_BYTES = 32 * 1024;
const COMPACTION_CHUNK_BYTES = 92 * 1024;
const COMPACTION_MARKER = "[Earlier conversation omitted during context compaction.]\n";
const FALLBACK_MARKER =
  "[Earlier conversation omitted because the selected model could not compact it.]\n";

const compactedByDraft = new Map<
  string,
  {
    selectionKey: string;
    source: AgentAttachment[];
    compacted: AgentAttachment[];
  }
>();

interface ModelSelection {
  cwd: string;
  provider: string;
  model: string | null;
  thinkingOptionId: string | null;
}

function utf8Suffix(bytes: Uint8Array, maxBytes: number): Uint8Array {
  let start = Math.max(0, bytes.length - maxBytes);
  while (start < bytes.length && (bytes[start]! & 0xc0) === 0x80) start += 1;
  return bytes.subarray(start);
}

function splitUtf8(bytes: Uint8Array, maxBytes: number): string[] {
  const decoder = new TextDecoder();
  const chunks: string[] = [];
  for (let start = 0; start < bytes.length; ) {
    let end = Math.min(bytes.length, start + maxBytes);
    while (end < bytes.length && (bytes[end]! & 0xc0) === 0x80) end -= 1;
    if (end < bytes.length && end - start > maxBytes / 2) {
      const newline = bytes.lastIndexOf(10, end - 1);
      if (newline > start) end = newline + 1;
    }
    chunks.push(decoder.decode(bytes.subarray(start, end)));
    start = end;
  }
  return chunks;
}

function recentBody(body: string, decoder: TextDecoder, encoder: TextEncoder): string {
  const bytes = encoder.encode(body);
  let tail = utf8Suffix(bytes, RECENT_BYTES);
  if (tail.length < bytes.length) {
    const newline = tail.indexOf(10);
    if (newline >= 0) tail = tail.subarray(newline + 1);
  }
  return decoder.decode(tail);
}

function parseChatHistory(text: string): { header: string; body: string; closing: string } | null {
  const separator = text.indexOf("\n\n");
  const closing = "</chat-history-summary>";
  if (separator < 0 || !text.endsWith(closing)) return null;
  return {
    header: text.slice(0, separator + 2),
    body: text.slice(separator + 2, -closing.length),
    closing,
  };
}

async function compactText(
  client: Pick<DaemonClient, "compactAgentContext">,
  context: string,
  selection: ModelSelection,
): Promise<string> {
  return client.compactAgentContext({ ...selection, context });
}

async function compactChatHistoryText(
  client: Pick<DaemonClient, "compactAgentContext">,
  text: string,
  selection: ModelSelection,
): Promise<string> {
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();
  const parsed = parseChatHistory(text);
  if (!parsed) return text;
  const bodyBytes = encoder.encode(parsed.body);
  if (bodyBytes.length <= COMPACT_AFTER_BYTES) return text;

  const recent = recentBody(parsed.body, decoder, encoder);
  const oldBytes = encoder.encode(parsed.body.slice(0, parsed.body.length - recent.length));
  try {
    const segments = splitUtf8(oldBytes, COMPACTION_CHUNK_BYTES);
    const summaries = await Promise.all(
      segments.map((segment) => compactText(client, segment, selection)),
    );
    const summary =
      summaries.length === 1
        ? summaries[0]!
        : await compactText(client, summaries.join("\n\n"), selection);
    return `${parsed.header}${COMPACTION_MARKER}${summary}\n\n[Most recent original messages]\n${recent}${parsed.closing}`;
  } catch {
    return `${parsed.header}${FALLBACK_MARKER}[Most recent original messages]\n${recent}${parsed.closing}`;
  }
}

export async function compactChatHistoryAttachments(
  client: Pick<DaemonClient, "compactAgentContext"> | null,
  draftId: string,
  attachments: AgentAttachment[] | undefined,
  selection: ModelSelection,
): Promise<AgentAttachment[] | undefined> {
  if (!client || !attachments?.length) return attachments;
  const historySource = attachments.filter(
    (attachment) => attachment.type === "text" && attachment.contextKind === "chat_history",
  );
  if (historySource.length === 0) return attachments;
  const selectionKey = JSON.stringify(selection);
  const cached = compactedByDraft.get(draftId);
  if (
    cached?.selectionKey === selectionKey &&
    cached.source.length === historySource.length &&
    cached.source.every((attachment, index) => attachment === historySource[index])
  ) {
    return cached.compacted;
  }
  let changed = false;
  const compacted = await Promise.all(
    attachments.map(async (attachment) => {
      if (attachment.type !== "text" || attachment.contextKind !== "chat_history") {
        return attachment;
      }
      const text = await compactChatHistoryText(client, attachment.text, selection);
      if (text === attachment.text) return attachment;
      changed = true;
      return { ...attachment, text };
    }),
  );
  if (!changed) return attachments;
  compactedByDraft.set(draftId, {
    selectionKey,
    source: historySource,
    compacted,
  });
  return compacted;
}

export function clearCompactedChatHistory(draftId: string): void {
  compactedByDraft.delete(draftId);
}

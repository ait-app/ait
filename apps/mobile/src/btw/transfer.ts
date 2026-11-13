import type { SideContextSnapshot, SideTopic } from "@ait/protocol/messages";
import type { ChatHistoryContextAttachment } from "@/attachments/types";
import { btwTopicTurns } from "./index";

/** Preserve the frozen parent context and every retained BTW turn in a normal fork attachment. */
export function buildBtwTransferAttachment(input: {
  draftId: string;
  serverId: string;
  agentId: string;
  snapshot: SideContextSnapshot;
  topic: SideTopic;
}): ChatHistoryContextAttachment {
  const { snapshot, topic } = input;
  const context = snapshot.entries.map((entry) => `### ${entry.role}\n\n${entry.text}`);
  const turns = btwTopicTurns(topic).flatMap((turn) => [
    `### user\n\n${turn.question}${
      turn.images?.length
        ? `\n\n${turn.images.map((image) => `![Attached image](data:${image.mimeType};base64,${image.data})`).join("\n\n")}`
        : ""
    }`,
    `### assistant\n\n${turn.answer}${turn.error ? `\n\n${turn.error}` : ""}`,
  ]);
  const text = [
    "# Conversation context",
    `Source session: ${snapshot.agentId}`,
    "## Parent conversation at the start of this side conversation",
    ...context,
    "## Side conversation",
    ...turns,
  ].join("\n\n");
  return {
    kind: "chat_history",
    id: `chat_history:${input.draftId}`,
    sideConversation: { topic },
    attachment: {
      type: "text",
      mimeType: "text/plain",
      contextKind: "chat_history",
      title: topic.title ?? topic.question,
      text,
    },
    source: {
      serverId: input.serverId,
      agentId: input.agentId,
      itemCount: snapshot.entries.length + turns.length,
    },
  };
}

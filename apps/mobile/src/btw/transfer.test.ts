import { expect, it } from "vitest";
import type { SideContextSnapshot, SideTopic } from "@ait/protocol/messages";
import { buildBtwTransferAttachment } from "./transfer";

const snapshot: SideContextSnapshot = {
  v: 1,
  agentId: "parent",
  provider: "custom-provider",
  model: "selected-model",
  capturedAt: 1,
  entries: [
    { role: "user", text: "Original context" },
    { role: "assistant", text: "Original answer" },
  ],
  truncated: false,
};

it("carries the frozen parent context and every follow-up into an editable fork attachment", () => {
  const topic: SideTopic = {
    id: "topic",
    question: "Root question",
    answer: "Root answer",
    status: "complete",
    createdAt: 1,
    updatedAt: 20,
    leafId: null,
    strategy: "isolated",
    truncated: false,
    followUps: Array.from({ length: 12 }, (_, index) => ({
      question: `Follow up ${index}`,
      answer: `Answer ${index}`,
      status: "complete",
      createdAt: index + 2,
      updatedAt: index + 3,
    })),
  };
  const attachment = buildBtwTransferAttachment({
    draftId: "draft",
    agentId: "parent",
    serverId: "server",
    snapshot,
    topic,
  });
  expect(attachment.attachment).toMatchObject({ type: "text", contextKind: "chat_history" });
  expect(attachment.attachment.text).toContain("Original context");
  expect(attachment.attachment.text).toContain("Original answer");
  expect(attachment.attachment.text).toContain("Root question");
  expect(attachment.attachment.text).toContain("Follow up 11");
  expect(attachment.attachment.text).toContain("Answer 11");
  expect(attachment.sideConversation?.topic).toBe(topic);
  expect(topic.followUps).toHaveLength(12);
  expect(snapshot.entries).toHaveLength(2);
});

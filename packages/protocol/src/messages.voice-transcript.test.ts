import { expect, it } from "vitest";
import { AgentTimelineItemPayloadSchema, WSOutboundMessageSchema } from "./messages.js";
import { validateWSOutboundMessage } from "./validation/ws-outbound.js";

it("preserves speech roles through schema and compiled stream validation", () => {
  const item = {
    type: "user_message",
    text: "test",
    messageId: "voice",
    voiceTranscript: [
      { role: "user", text: "hello" },
      { role: "assistant", text: "hi" },
      { role: "unknown", text: "…earlier speech" },
    ],
  };
  const message = {
    type: "session",
    message: {
      type: "agent.stream",
      payload: {
        agentId: "agent",
        timestamp: "2026-10-25T00:00:00.000Z",
        event: { type: "timeline", provider: "codex", item },
      },
    },
  };
  expect(AgentTimelineItemPayloadSchema.parse(item)).toEqual(item);
  expect(WSOutboundMessageSchema.parse(message)).toEqual(message);
  expect(validateWSOutboundMessage(message)).toEqual({ success: true, data: message });
  expect(
    AgentTimelineItemPayloadSchema.safeParse({
      ...item,
      voiceTranscript: [{ role: "invalid", text: "hello" }],
    }).success,
  ).toBe(false);
});

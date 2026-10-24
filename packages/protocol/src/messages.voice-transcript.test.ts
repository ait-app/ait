import { expect, it } from "vitest";
import { AgentTimelineItemPayloadSchema, WSOutboundMessageSchema } from "./messages.js";
import { validateWSOutboundMessage } from "./validation/ws-outbound.js";
import { timelineItemIdentity } from "./timeline-identity.js";

it.each([
  {
    type: "voice",
    kind: "transcript",
    id: "u",
    sessionId: "call",
    role: "user",
    text: "hello",
    status: "streaming",
  },
  {
    type: "voice",
    kind: "transcript",
    id: "a",
    sessionId: "call",
    role: "assistant",
    text: "reply",
    status: "completed",
  },
  { type: "voice", kind: "session", id: "end", sessionId: "call", status: "ended" },
])("preserves canonical voice through compiled stream validation: $kind $status", (item) => {
  const parsed = AgentTimelineItemPayloadSchema.parse(item);
  expect(parsed).toEqual(item);
  expect(timelineItemIdentity(parsed)).toBe(`voice:call/${item.id}`);
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
  expect(validateWSOutboundMessage(message)).toEqual({ success: true, data: message });
});

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

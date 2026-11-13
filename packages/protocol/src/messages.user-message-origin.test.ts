import { expect, it } from "vitest";
import type { UserMessageOrigin } from "./agent-types.js";
import { AgentTimelineItemPayloadSchema, WSOutboundMessageSchema } from "./messages.js";
import { validateWSOutboundMessage } from "./validation/ws-outbound.js";

const origins: UserMessageOrigin[] = [
  { kind: "schedule", scheduleId: "s1", runId: "r1", name: "nightly" },
  { kind: "schedule", scheduleId: "s1", runId: "r1" },
  { kind: "bot_wake" },
  { kind: "bot_instructions" },
];

it.each(origins)("preserves the Ait origin through compiled stream validation: %j", (origin) => {
  const item = {
    type: "user_message",
    text: "Run the nightly report",
    messageId: "m1",
    origin,
  };
  const message = {
    type: "session",
    message: {
      type: "agent.stream",
      payload: {
        agentId: "agent",
        timestamp: "2026-11-13T00:00:00.000Z",
        event: { type: "timeline", provider: "codex", item },
      },
    },
  };
  expect(AgentTimelineItemPayloadSchema.parse(item)).toEqual(item);
  expect(WSOutboundMessageSchema.parse(message)).toEqual(message);
  expect(validateWSOutboundMessage(message)).toEqual({ success: true, data: message });
});

it("rejects an origin no Ait component writes", () => {
  const item = {
    type: "user_message",
    text: "Run the nightly report",
    origin: { kind: "schedule" },
  };
  expect(AgentTimelineItemPayloadSchema.safeParse(item).success).toBe(false);
  expect(
    AgentTimelineItemPayloadSchema.safeParse({
      ...item,
      origin: { kind: "unknown" },
    }).success,
  ).toBe(false);
  const message = {
    type: "session",
    message: {
      type: "agent.stream",
      payload: {
        agentId: "agent",
        timestamp: "2026-11-13T00:00:00.000Z",
        event: { type: "timeline", provider: "codex", item },
      },
    },
  };
  expect(validateWSOutboundMessage(message).success).toBe(false);
});

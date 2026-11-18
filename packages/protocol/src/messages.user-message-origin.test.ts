import { expect, it } from "vitest";
import type { UserMessageOrigin } from "./agent-types.js";
import { AgentTimelineItemPayloadSchema, WSOutboundMessageSchema } from "./messages.js";
import { validateWSOutboundMessage } from "./validation/ws-outbound.js";

const origins: UserMessageOrigin[] = [
  { kind: "schedule", scheduleId: "s1", runId: "r1", name: "nightly" },
  { kind: "schedule", scheduleId: "s1", runId: "r1" },
  { kind: "bot_wake" },
  { kind: "bot_instructions" },
  { kind: "ticket_pull", ticketId: "t1", pull: 12 },
  { kind: "ticket_answer", ticketId: "t1" },
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

it("rejects a malformed origin of a known kind", () => {
  const item = {
    type: "user_message",
    text: "Run the nightly report",
    origin: { kind: "schedule" },
  };
  expect(AgentTimelineItemPayloadSchema.safeParse(item).success).toBe(false);
  expect(validateWSOutboundMessage(stream(item)).success).toBe(false);
});

it("keeps an item whose origin kind a newer daemon added, without failing its message", () => {
  const item = {
    type: "user_message",
    text: "Review comments arrived",
    origin: { kind: "some_future_kind", extra: 1 },
  };
  const projected = { ...item, origin: { kind: "other", other: "some_future_kind" } };

  expect(AgentTimelineItemPayloadSchema.parse(item)).toEqual(projected);
  const validated = validateWSOutboundMessage(stream(item));
  expect(validated).toEqual({ success: true, data: stream(projected) });
});

function stream(item: Record<string, unknown>) {
  return {
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
}

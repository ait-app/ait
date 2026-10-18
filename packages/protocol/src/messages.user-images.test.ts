import { expect, it } from "vitest";
import { AgentTimelineItemPayloadSchema, WSOutboundMessageSchema } from "./messages.js";
import { validateWSOutboundMessage } from "./validation/ws-outbound.js";

it("preserves user image sources through schema and compiled stream validation", () => {
  const item = {
    type: "user_message",
    text: "",
    messageId: "user",
    imageSources: ["/host/images/plant.png"],
  };
  expect(AgentTimelineItemPayloadSchema.parse(item)).toEqual(item);
  const message = {
    type: "session",
    message: {
      type: "agent.stream",
      payload: {
        agentId: "agent",
        timestamp: "2026-10-18T00:00:00.000Z",
        event: { type: "timeline", provider: "codex", item },
      },
    },
  };
  expect(WSOutboundMessageSchema.parse(message)).toEqual(message);
  expect(validateWSOutboundMessage(message)).toEqual({ success: true, data: message });
});

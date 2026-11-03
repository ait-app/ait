import { expect, test } from "vitest";
import { readFileSync } from "node:fs";
import { AgentTimelineItemPayloadSchema } from "./messages.js";
import { validateWSOutboundMessage } from "./validation/ws-outbound.js";
import { timelineItemIdentity } from "./timeline-identity.js";

test("native goal cards and original control actions survive generated wire validation", () => {
  const item = JSON.parse(
    readFileSync(
      new URL("../../../crates/provider/tests/fixtures/dsh_goal_control.json", import.meta.url),
      "utf8",
    ),
  );
  expect(AgentTimelineItemPayloadSchema.parse(item)).toEqual(item);
  expect(timelineItemIdentity(item)).toBe(`provider-command:${item.id}`);
  const envelope = {
    type: "session",
    message: {
      type: "agent.stream",
      payload: {
        agentId: "agent",
        event: { type: "timeline", provider: "deepseek-harness", item },
        timestamp: "2026-10-31T00:00:00Z",
      },
    },
  };
  expect(validateWSOutboundMessage(envelope)).toEqual({ success: true, data: envelope });
});

test("delivered native archive metadata survives generated wire validation", () => {
  const item = JSON.parse(
    readFileSync(
      new URL("../../../crates/provider/tests/fixtures/dsh_export_control.json", import.meta.url),
      "utf8",
    ),
  );
  expect(AgentTimelineItemPayloadSchema.parse(item)).toEqual(item);
  const envelope = {
    type: "session",
    message: {
      type: "agent.stream",
      payload: {
        agentId: "agent",
        event: { type: "timeline", provider: "deepseek-harness", item },
        timestamp: "2026-11-02T00:00:00Z",
      },
    },
  };
  expect(validateWSOutboundMessage(envelope)).toEqual({ success: true, data: envelope });
  for (const fileName of [
    "../outside.zip",
    "a/b.zip",
    "a\\b.zip",
    ".",
    "..",
    "a\u0000.zip",
    "a\u001f.zip",
    "a\u007f.zip",
  ]) {
    const invalid = { ...item, file: { ...item.file, fileName } };
    expect(AgentTimelineItemPayloadSchema.safeParse(invalid).success).toBe(false);
    expect(
      validateWSOutboundMessage({
        ...envelope,
        message: {
          ...envelope.message,
          payload: {
            ...envelope.message.payload,
            event: { ...envelope.message.payload.event, item: invalid },
          },
        },
      }).success,
    ).toBe(false);
  }
  const unicode = { ...item, file: { ...item.file, fileName: "会话记录.zip" } };
  expect(AgentTimelineItemPayloadSchema.parse(unicode)).toEqual(unicode);
  const unicodeEnvelope = {
    ...envelope,
    message: {
      ...envelope.message,
      payload: {
        ...envelope.message.payload,
        event: { ...envelope.message.payload.event, item: unicode },
      },
    },
  };
  expect(validateWSOutboundMessage(unicodeEnvelope)).toEqual({
    success: true,
    data: unicodeEnvelope,
  });
});

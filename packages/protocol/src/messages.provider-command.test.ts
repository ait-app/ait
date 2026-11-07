import { z } from "zod";
import { expect, test } from "vitest";
import { readFileSync } from "node:fs";
import {
  AgentTimelineItemPayloadSchema,
  FetchAgentTimelineResponseMessageSchema,
} from "./messages.js";
import { validateWSOutboundMessage } from "./validation/ws-outbound.js";
import { timelineItemIdentity, unwrapTimelineItem } from "./timeline-identity.js";

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

test("compatible native results parse in legacy closed item unions and rich live/history schemas", () => {
  const cases = JSON.parse(
    readFileSync(
      new URL(
        "../../../crates/provider/tests/fixtures/provider_command_compat.json",
        import.meta.url,
      ),
      "utf8",
    ),
  );
  // Exact assistant variant from 850e0075; old object schemas strip additive metadata.
  const legacyItem = z.union([
    z.object({
      type: z.literal("assistant_message"),
      text: z.string(),
      messageId: z.string().optional(),
    }),
    z.object({ type: z.literal("reasoning"), text: z.string() }),
  ]);
  for (const { native, wire } of cases) {
    expect(legacyItem.safeParse(native).success).toBe(false);
    expect(legacyItem.parse(wire)).toEqual({
      type: "assistant_message",
      text: native.text,
      messageId: `provider-command:${native.id}`,
    });
    expect(unwrapTimelineItem(AgentTimelineItemPayloadSchema.parse(wire))).toEqual(native);
    const event = {
      type: "session",
      message: {
        type: "agent.stream",
        payload: {
          agentId: "agent",
          event: { type: "timeline", provider: "deepseek-harness", item: wire },
          timestamp: "2026-11-04T00:00:00Z",
        },
      },
    };
    expect(validateWSOutboundMessage(event).success).toBe(true);
    const entry = {
      provider: "deepseek-harness",
      item: wire,
      timestamp: "2026-11-04T00:00:00Z",
      seqStart: 1,
      seqEnd: 1,
      sourceSeqRanges: [{ startSeq: 1, endSeq: 1 }],
      collapsed: [],
    };
    const page = {
      type: "agent.timeline.get.response",
      payload: {
        requestId: "page",
        agentId: "agent",
        agent: null,
        direction: "tail",
        projection: "projected",
        epoch: "epoch",
        reset: false,
        staleCursor: false,
        gap: false,
        window: { minSeq: 1, maxSeq: 1, nextSeq: 2 },
        startCursor: { epoch: "epoch", seq: 1 },
        endCursor: { epoch: "epoch", seq: 1 },
        hasOlder: false,
        hasNewer: false,
        entries: [entry],
        error: null,
      },
    };
    expect(FetchAgentTimelineResponseMessageSchema.safeParse(page).success).toBe(true);
    expect(
      z.object({ entries: z.array(z.object({ item: legacyItem })) }).safeParse(page.payload)
        .success,
    ).toBe(true);
  }
});

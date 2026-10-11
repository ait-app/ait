import { expect, test } from "vitest";
import {
  FetchAgentTimelineRequestMessageSchema,
  FetchAgentTimelineResponseMessageSchema,
} from "./messages";
import { validateWSOutboundMessage } from "./validation/ws-outbound";

test("preserves the optional cached read preference without changing older requests", () => {
  const request = { type: "agent.timeline.get.request", requestId: "read", agentId: "agent" };
  for (const allowCached of [undefined, false, true]) {
    const value = { ...request, ...(allowCached === undefined ? {} : { allowCached }) };
    expect(FetchAgentTimelineRequestMessageSchema.parse(value)).toEqual(value);
  }
});

test("preserves native revalidation hints in both response validators", () => {
  const payload = {
    requestId: "read",
    agentId: "agent",
    agent: null,
    direction: "tail",
    projection: "projected",
    epoch: "epoch",
    reset: false,
    staleCursor: false,
    gap: false,
    window: { minSeq: 0, maxSeq: 0, nextSeq: 1 },
    startCursor: null,
    endCursor: null,
    hasOlder: false,
    hasNewer: false,
    entries: [],
    error: null,
  };
  for (const historyNeedsRefresh of [undefined, false, true]) {
    const message = {
      type: "agent.timeline.get.response",
      payload: {
        ...payload,
        ...(historyNeedsRefresh === undefined ? {} : { historyNeedsRefresh }),
      },
    };
    expect(FetchAgentTimelineResponseMessageSchema.parse(message)).toEqual(message);
    const envelope = { type: "session", message };
    expect(validateWSOutboundMessage(envelope)).toMatchObject({ success: true, data: envelope });
  }
});

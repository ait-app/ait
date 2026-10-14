import { describe, expect, it } from "vitest";
import {
  SessionInboundMessageSchema,
  SessionOutboundMessageSchema,
  StatusMessageSchema,
} from "./messages.js";

describe("plugin timeline items", () => {
  it("parses plugin timeline append messages and advertises the capability", () => {
    expect(
      SessionInboundMessageSchema.parse({
        type: "agent.timeline.append.request",
        requestId: "request-append",
        agentId: "agent-1",
        item: {
          type: "plugin",
          id: "review-1",
          kind: "review",
          version: 1,
          data: { status: "running" },
        },
      }),
    ).toMatchObject({ type: "agent.timeline.append.request" });
    expect(
      SessionOutboundMessageSchema.parse({
        type: "agent.timeline.append.response",
        payload: { requestId: "request-append", seq: 7, epoch: "epoch-1" },
      }),
    ).toMatchObject({ type: "agent.timeline.append.response" });
    expect(
      StatusMessageSchema.parse({
        type: "status",
        payload: {
          status: "server_info",
          serverId: "server-1",
          features: { pluginTimelineItems: true },
        },
      }).payload,
    ).toMatchObject({ features: { pluginTimelineItems: true } });
  });

  it.each([0, -1, 1.5])("rejects plugin timeline version %s", (version) => {
    expect(() =>
      SessionInboundMessageSchema.parse({
        type: "agent.timeline.append.request",
        requestId: "append-1",
        agentId: "agent-1",
        item: { type: "plugin", id: "row-1", kind: "review", version, data: {} },
      }),
    ).toThrow();
  });
});

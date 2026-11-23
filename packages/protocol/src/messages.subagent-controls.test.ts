import { describe, expect, it } from "vitest";
import {
  ServerInfoStatusPayloadSchema,
  SessionInboundMessageSchema,
  SessionOutboundMessageSchema,
} from "./messages";
import { validateWSOutboundMessage } from "./validation/ws-outbound";

describe("observed child controls", () => {
  const guide = {
    type: "agent.subagents.guide.request",
    requestId: "request-1",
    parentAgentId: "parent",
    childAgentId: "child",
    message: "  investigate\n",
    expectedRevision: 4,
  };

  it("keeps guidance verbatim and accepts the optional revision fence", () => {
    expect(SessionInboundMessageSchema.parse(guide)).toEqual(guide);
    const stop = {
      type: "agent.subagents.stop.request",
      requestId: "request-2",
      parentAgentId: "parent",
      childAgentId: "child",
      expectedRevision: 4,
    };
    expect(SessionInboundMessageSchema.parse(stop)).toEqual(stop);
  });

  it.each(["", " \n", "界".repeat(21846)])(
    "rejects blank or overlong UTF-8 guidance",
    (message) => {
      expect(SessionInboundMessageSchema.safeParse({ ...guide, message }).success).toBe(false);
    },
  );

  it("accepts exactly the 64 KiB UTF-8 boundary and rejects invalid revisions", () => {
    expect(
      SessionInboundMessageSchema.safeParse({ ...guide, message: "界".repeat(21845) + "x" })
        .success,
    ).toBe(true);
    expect(SessionInboundMessageSchema.safeParse({ ...guide, expectedRevision: -1 }).success).toBe(
      false,
    );
  });

  it.each([
    ["guide", "accepted"],
    ["guide", "not_running"],
    ["stop", "cancelled"],
    ["stop", "not_running"],
  ])("parses %s receipt with disposition %s", (method, disposition) => {
    const response = {
      type: `agent.subagents.${method}.response`,
      payload: {
        requestId: "request-1",
        parentAgentId: "parent",
        childAgentId: "child",
        disposition,
        error: null,
      },
    };
    expect(SessionOutboundMessageSchema.parse(response)).toEqual(response);
    expect(validateWSOutboundMessage({ type: "session", message: response }).success).toBe(true);
  });

  it("keeps control support optional for old hosts", () => {
    const status = { status: "server_info", serverId: "old", features: {} };
    expect(ServerInfoStatusPayloadSchema.parse(status).features.subagentControls).toBeUndefined();
  });
});

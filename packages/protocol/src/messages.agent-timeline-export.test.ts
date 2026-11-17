import { describe, expect, it } from "vitest";
import {
  AgentTimelineExportResultSchema,
  ServerInfoStatusPayloadSchema,
  SessionInboundMessageSchema,
  SessionOutboundMessageSchema,
} from "./messages";
import { validateWSOutboundMessage } from "./validation/ws-outbound";

const artifact = {
  agentId: "agent-1",
  format: "markdown",
  scope: "conversation",
  source: "persisted_timeline",
  token: "single-use-token",
  fileName: "conversation.md",
  mimeType: "text/markdown; charset=utf-8",
  size: 1024,
  entries: 2,
  expiresAt: "2026-11-11T00:05:00Z",
};

describe("persisted timeline export", () => {
  it.each([
    ["markdown", "conversation"],
    ["markdown", "timeline"],
    ["html", "conversation"],
    ["html", "timeline"],
  ])("registers %s / %s requests and direct results", (format, scope) => {
    const request = {
      type: "agent.timeline.export.request",
      requestId: "request-1",
      agentId: "agent-1",
      format,
      scope,
    };
    expect(SessionInboundMessageSchema.parse(request)).toEqual(request);
    const result = {
      ...artifact,
      format,
      scope,
      fileName: format === "html" ? "timeline.html" : "timeline.md",
      mimeType: format === "html" ? "text/html; charset=utf-8" : artifact.mimeType,
    };
    expect(AgentTimelineExportResultSchema.parse(result)).toEqual(result);
    const response = {
      type: "agent.timeline.export.response",
      payload: { ...result, requestId: "request-1" },
    };
    expect(SessionOutboundMessageSchema.parse(response)).toEqual(response);
    expect(validateWSOutboundMessage({ type: "session", message: response }).success).toBe(true);
  });

  it.each([
    { format: "dshzip" },
    { format: "native_html" },
    { scope: "runtime" },
    { source: "native_session" },
    { size: 16 * 1024 * 1024 + 1 },
    { size: -1 },
    { entries: 0.5 },
  ])("rejects unsupported or invalid artifact metadata %j", (invalid) => {
    expect(AgentTimelineExportResultSchema.safeParse({ ...artifact, ...invalid }).success).toBe(
      false,
    );
  });

  it("accepts empty artifacts and the exact byte limit", () => {
    expect(
      AgentTimelineExportResultSchema.parse({ ...artifact, size: 0, entries: 0 }).entries,
    ).toBe(0);
    expect(
      AgentTimelineExportResultSchema.parse({ ...artifact, size: 16 * 1024 * 1024 }).size,
    ).toBe(16 * 1024 * 1024);
  });

  it.each([true, false])("registers token cancellation with released=%s", (released) => {
    const request = {
      type: "agent.timeline.export.cancel.request",
      requestId: "cancel-1",
      agentId: "agent-1",
      token: artifact.token,
    };
    expect(SessionInboundMessageSchema.parse(request)).toEqual(request);
    const response = {
      type: "agent.timeline.export.cancel.response",
      payload: { requestId: "cancel-1", released },
    };
    expect(SessionOutboundMessageSchema.parse(response)).toEqual(response);
    expect(validateWSOutboundMessage({ type: "session", message: response }).success).toBe(true);
    expect(SessionInboundMessageSchema.safeParse({ ...request, token: undefined }).success).toBe(
      false,
    );
  });

  it("retains old host schemas without export support", () => {
    expect(
      ServerInfoStatusPayloadSchema.parse({
        status: "server_info",
        serverId: "old-host",
        features: {},
      }).features.agentTimelineExport,
    ).toBeUndefined();
  });
});

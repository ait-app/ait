import { describe, expect, it } from "vitest";
import {
  AgentNativeHtmlExportResultSchema,
  ServerInfoStatusPayloadSchema,
  SessionInboundMessageSchema,
  SessionOutboundMessageSchema,
} from "./messages";
import { validateWSOutboundMessage } from "./validation/ws-outbound";

const artifact = {
  agentId: "agent-1",
  source: "omp_native",
  token: "single-use-token",
  fileName: "ait-native-123.html",
  mimeType: "text/html; charset=utf-8",
  size: 4096,
  expiresAt: "2026-11-11T00:05:00Z",
};

describe("omp native html export", () => {
  it("registers the request and direct result", () => {
    const request = {
      type: "agent.native_html.export.request",
      requestId: "request-1",
      agentId: "agent-1",
      operationId: "00000000-0000-4000-8000-000000000001",
    };
    expect(SessionInboundMessageSchema.parse(request)).toEqual(request);
    expect(AgentNativeHtmlExportResultSchema.parse(artifact)).toEqual(artifact);
    const response = {
      type: "agent.native_html.export.response",
      payload: { ...artifact, requestId: "request-1" },
    };
    expect(SessionOutboundMessageSchema.parse(response)).toEqual(response);
    expect(validateWSOutboundMessage({ type: "session", message: response }).success).toBe(true);
  });

  it("carries no generic timeline options on the native request", () => {
    const request = {
      type: "agent.native_html.export.request",
      requestId: "request-1",
      agentId: "agent-1",
      operationId: "00000000-0000-4000-8000-000000000001",
      format: "html",
      scope: "timeline",
    };
    const parsed = SessionInboundMessageSchema.parse(request);
    // The native request has no format/scope; any such fields are stripped, never honored.
    expect(parsed).toEqual({
      type: "agent.native_html.export.request",
      requestId: "request-1",
      agentId: "agent-1",
      operationId: "00000000-0000-4000-8000-000000000001",
    });
  });

  it.each([
    { source: "persisted_timeline" },
    { source: "native_session" },
    { size: 16 * 1024 * 1024 + 1 },
    { size: -1 },
    { size: 1.5 },
  ])("rejects unsupported or invalid native artifact metadata %j", (invalid) => {
    expect(AgentNativeHtmlExportResultSchema.safeParse({ ...artifact, ...invalid }).success).toBe(
      false,
    );
  });

  it("accepts empty artifacts and the exact byte limit", () => {
    expect(AgentNativeHtmlExportResultSchema.parse({ ...artifact, size: 0 }).size).toBe(0);
    expect(
      AgentNativeHtmlExportResultSchema.parse({ ...artifact, size: 16 * 1024 * 1024 }).size,
    ).toBe(16 * 1024 * 1024);
  });

  it.each([true, false])("registers token cancellation with released=%s", (released) => {
    const request = {
      type: "agent.native_html.export.cancel.request",
      requestId: "cancel-1",
      agentId: "agent-1",
      token: artifact.token,
    };
    expect(SessionInboundMessageSchema.parse(request)).toEqual(request);
    const response = {
      type: "agent.native_html.export.cancel.response",
      payload: { requestId: "cancel-1", released },
    };
    expect(SessionOutboundMessageSchema.parse(response)).toEqual(response);
    expect(validateWSOutboundMessage({ type: "session", message: response }).success).toBe(true);
    expect(SessionInboundMessageSchema.safeParse({ ...request, token: undefined }).success).toBe(
      false,
    );
  });

  it("requires exactly one cancellation target", () => {
    const request = {
      type: "agent.native_html.export.cancel.request",
      requestId: "cancel-1",
      agentId: "agent-1",
    };
    const operationId = "00000000-0000-4000-8000-000000000001";
    expect(SessionInboundMessageSchema.safeParse({ ...request, operationId }).success).toBe(true);
    expect(SessionInboundMessageSchema.safeParse(request).success).toBe(false);
    expect(
      SessionInboundMessageSchema.safeParse({ ...request, operationId, token: "x" }).success,
    ).toBe(false);
    expect(SessionInboundMessageSchema.safeParse({ ...request, operationId: "bad" }).success).toBe(
      false,
    );
    expect(
      SessionInboundMessageSchema.safeParse({
        ...request,
        operationId: "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA",
      }).success,
    ).toBe(false);
  });

  it("retains old host schemas without native export support", () => {
    expect(
      ServerInfoStatusPayloadSchema.parse({
        status: "server_info",
        serverId: "old-host",
        features: {},
      }).features.agentNativeHtmlExport,
    ).toBeUndefined();
  });
});

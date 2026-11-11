import { describe, expect, it } from "vitest";
import {
  AgentSessionStatisticsSchema,
  ServerInfoStatusPayloadSchema,
  SessionInboundMessageSchema,
  SessionOutboundMessageSchema,
} from "./messages";
import { validateWSOutboundMessage } from "./validation/ws-outbound";

const statistics = {
  agentId: "agent-1",
  provider: "omp",
  status: "idle",
  configuredModel: null,
  effectiveModel: "provider-model",
  updatedAt: "2026-11-11T00:00:00Z",
  usageScope: "provider_snapshot",
  usage: {
    inputTokens: null,
    cachedInputTokens: 0,
    outputTokens: 0,
    totalCostUsd: null,
    contextWindowMaxTokens: 200000,
    contextWindowUsedTokens: null,
  },
};

describe("agent session statistics", () => {
  it("preserves unknown and known-zero values without deriving totals", () => {
    expect(AgentSessionStatisticsSchema.parse(statistics)).toEqual(statistics);
    const response = {
      type: "agent.statistics.get.response",
      payload: { ...statistics, requestId: "request-1" },
    };
    expect(SessionOutboundMessageSchema.parse(response)).toEqual(response);
    expect(validateWSOutboundMessage({ type: "session", message: response }).success).toBe(true);
  });

  it.each(["session_total", "last_turn", "future_scope"])(
    "rejects unknown scope %s",
    (usageScope) => {
      expect(AgentSessionStatisticsSchema.safeParse({ ...statistics, usageScope }).success).toBe(
        false,
      );
    },
  );

  it("registers the read and permits old hosts without the capability", () => {
    const request = {
      type: "agent.statistics.get.request",
      requestId: "request-1",
      agentId: "agent-1",
    };
    expect(SessionInboundMessageSchema.parse(request)).toEqual(request);
    expect(
      ServerInfoStatusPayloadSchema.parse({
        status: "server_info",
        serverId: "old-host",
        features: {},
      }).features.agentStatisticsGet,
    ).toBeUndefined();
  });
});

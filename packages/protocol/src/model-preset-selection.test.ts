import { describe, expect, it } from "vitest";
import {
  AgentModelPresetContextResponseSchema,
  SelectAgentModelPresetRequestSchema,
  CreateAgentRequestMessageSchema,
} from "./messages";
import { validateWSOutboundMessage } from "./validation/ws-outbound";

describe("model preset identity", () => {
  it("preserves the owner-only preset context and source without exposing a path", () => {
    const message = {
      type: "agent.model_preset.context.response",
      payload: {
        requestId: "r",
        agentId: "a",
        provider: "omp",
        presets: [{ name: "Work", source: "project", model: "fast" }],
        selectedModelPreset: { name: "Work", source: "project" },
        selectedAvailable: true,
        error: null,
      },
    };
    expect(AgentModelPresetContextResponseSchema.parse(message)).toEqual(message);
    expect(validateWSOutboundMessage({ type: "session", message }).success).toBe(true);
  });
  it("accepts explicit source for selection and creation baselines", () => {
    expect(
      SelectAgentModelPresetRequestSchema.parse({
        type: "agent.model_preset.select.request",
        requestId: "r",
        agentId: "a",
        name: "Work",
        source: "global",
      }).source,
    ).toBe("global");
    const request = CreateAgentRequestMessageSchema.parse({
      type: "agent.create.request",
      requestId: "r",
      config: { provider: "omp", cwd: "/repo" },
      modelPreset: {
        name: "Work",
        source: "project",
        baseFeatureValues: { "modelRole.worker": "old" },
        baseThinkingOptionId: "low",
      },
    });
    expect(request.modelPreset?.baseThinkingOptionId).toBe("low");
  });
});

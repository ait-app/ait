import { SetAgentFeatureResponseMessageSchema } from "./messages";
import { describe, expect, it } from "vitest";

describe("native-confirmed configuration response compatibility", () => {
  const base = {
    type: "agent.feature.set.response" as const,
    payload: {
      requestId: "request-1",
      agentId: "agent-1",
      accepted: false,
      error: "provider rejected configuration fields: featureValues.fastMode",
    },
  };

  it("accepts the optional rejectedFields list from new hosts", () => {
    const parsed = SetAgentFeatureResponseMessageSchema.parse({
      ...base,
      payload: { ...base.payload, rejectedFields: ["featureValues.fastMode"] },
    });
    expect(parsed.payload.rejectedFields).toEqual(["featureValues.fastMode"]);
  });

  it("keeps old-host responses without rejectedFields valid", () => {
    const parsed = SetAgentFeatureResponseMessageSchema.parse(base);
    expect(parsed.payload.rejectedFields).toBeUndefined();
    expect(parsed.payload.accepted).toBe(false);
  });

  it("rejects malformed rejectedFields entries", () => {
    expect(() =>
      SetAgentFeatureResponseMessageSchema.parse({
        ...base,
        payload: { ...base.payload, rejectedFields: [42] },
      }),
    ).toThrow();
  });
});

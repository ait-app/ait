import { describe, expect, it } from "vitest";
import type { AgentFeature } from "@ait/protocol/agent-types";
import { liveAgentFeatures, mergeRoleChoices } from "./role-choices";

describe("restored native role controls", () => {
  const saved: AgentFeature[] = [
    { type: "toggle", id: "autoRetry", label: "Retry", value: true },
    {
      type: "select",
      id: "modelRole.advisor",
      label: "Saved advisor",
      value: "old/model:high",
      options: [
        { id: "native", label: "Inherit" },
        { id: "old/model:high", label: "Saved model" },
      ],
    },
  ];
  const current: AgentFeature[] = [
    {
      type: "select",
      id: "modelRole.advisor",
      label: "Advisor model",
      value: "native",
      options: [
        { id: "native", label: "Use OMP setting (native/default)" },
        { id: "new/model", label: "New model" },
      ],
    },
  ];

  it("offers current native models after restart without replacing the saved model or its effort", () => {
    const result = mergeRoleChoices(saved, current)!;
    expect(result[0]).toBe(saved[0]);
    expect(result[1]).toMatchObject({
      label: "Advisor model",
      value: "old/model:high",
      options: [
        ...(current[0].type === "select" ? current[0].options : []),
        { id: "old/model:high", label: "Saved model" },
      ],
    });
    expect(saved[1].label).toBe("Saved advisor");
  });

  it("keeps the stored controls when native discovery is unavailable", () => {
    expect(mergeRoleChoices(saved, undefined)).toBe(saved);
    expect(mergeRoleChoices(saved, [])).toEqual(saved);
  });

  it("shows the catalog to a session that saved no controls at all", () => {
    // A session persisted before the provider exposed roles still has to show them.
    expect(mergeRoleChoices(undefined, current)).toEqual(current);
    expect(mergeRoleChoices(undefined, undefined)).toBeUndefined();
  });

  it("includes newly discovered custom roles and preserves unrelated controls", () => {
    const custom: AgentFeature = {
      type: "select",
      id: "modelRole.custom",
      label: "Custom",
      value: "native",
      options: [],
    };
    const result = mergeRoleChoices(saved, [...current, custom])!;
    expect(result).toHaveLength(3);
    expect(result[0]).toBe(saved[0]);
    expect(result[2]).toBe(custom);
  });
});

describe("live agent features", () => {
  const preset = (name: string): AgentFeature => ({
    type: "select",
    id: "modelPreset",
    label: "Preset",
    value: "__custom__",
    options: [
      { id: "__custom__", label: "Custom" },
      { id: name, label: name, metadata: { configuration: { modelId: "claude-opus-5" } } },
    ],
  });

  it("discovers a preset created after the session was saved", () => {
    // The stored list has no preset feature at all; only the catalog knows about the new one.
    const saved: AgentFeature[] = [
      { type: "toggle", id: "autoRetry", label: "Retry", value: true },
    ];
    const result = liveAgentFeatures(saved, [preset("team")], {
      modelId: "claude-opus-5",
      thinkingOptionId: null,
    })!;
    expect(result.map((feature) => feature.id)).toEqual(["autoRetry", "modelPreset"]);
    // The label is recomputed from the effective selection, so the created preset reads as chosen.
    expect(result[1].type === "select" && result[1].value).toBe("team");
  });

  it("keeps the stored list untouched when the catalog has no model configuration", () => {
    const saved: AgentFeature[] = [
      { type: "toggle", id: "autoRetry", label: "Retry", value: true },
    ];
    expect(liveAgentFeatures(saved, [], { modelId: "claude-opus-5" })).toEqual(saved);
  });
});

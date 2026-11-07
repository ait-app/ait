import { describe, expect, it } from "vitest";
import type { AgentFeature } from "@ait/protocol/agent-types";
import { mergeRoleChoices } from "./role-choices";

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
    expect(mergeRoleChoices(undefined, current)).toBeUndefined();
    expect(mergeRoleChoices(saved, [])).toEqual(saved);
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

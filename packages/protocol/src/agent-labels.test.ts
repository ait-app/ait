import { describe, expect, test } from "vitest";
import {
  getParentAgentIdFromLabels,
  getOpenAgentTabLabel,
  isOpenAgentTabLabel,
  PARENT_AGENT_ID_LABEL,
} from "./agent-labels.js";

describe("agent label policy", () => {
  test("treats a non-empty parent agent label as delegation", () => {
    const labels = { [PARENT_AGENT_ID_LABEL]: " parent-agent \n" };

    expect(getParentAgentIdFromLabels(labels)).toBe("parent-agent");
  });

  test("ignores missing, empty, and non-string parent agent labels", () => {
    expect(getParentAgentIdFromLabels({})).toBeNull();
    expect(getParentAgentIdFromLabels({ [PARENT_AGENT_ID_LABEL]: "   " })).toBeNull();
    expect(getParentAgentIdFromLabels({ [PARENT_AGENT_ID_LABEL]: 42 })).toBeNull();
  });

  test("recognizes only client-scoped open-tab labels", () => {
    expect(isOpenAgentTabLabel(getOpenAgentTabLabel("client-a"))).toBe(true);
    expect(isOpenAgentTabLabel("paseo.open-agent-tab")).toBe(false);
    expect(isOpenAgentTabLabel("custom.open-agent-tab.client-a")).toBe(false);
  });
});

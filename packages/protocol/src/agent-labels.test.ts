import { describe, expect, test } from "vitest";
import {
  getParentAgentIdFromLabels,
  getOpenAgentTabLabel,
  getRemoteParentFromLabels,
  isOpenAgentTabLabel,
  PARENT_AGENT_ID_LABEL,
  PARENT_SERVER_ID_LABEL,
  REMOTE_PARENT_AGENT_ID_LABEL,
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

  test("reads a remote parent only when both labels are present", () => {
    expect(PARENT_SERVER_ID_LABEL).toBe("ait.parent.server-id");
    expect(REMOTE_PARENT_AGENT_ID_LABEL).toBe("ait.parent.agent-id");
    expect(
      getRemoteParentFromLabels({ "ait.parent.server-id": "s", "ait.parent.agent-id": "a" }),
    ).toEqual({ serverId: "s", agentId: "a" });
    expect(getRemoteParentFromLabels({ "ait.parent.agent-id": "a" })).toBeNull();
    expect(getRemoteParentFromLabels({ "ait.parent.server-id": "s" })).toBeNull();
    expect(getRemoteParentFromLabels({ "paseo.parent-agent-id": "a" })).toBeNull();
  });

  test("ignores empty, blank and non-string remote parent labels", () => {
    expect(
      getRemoteParentFromLabels({ "ait.parent.server-id": "", "ait.parent.agent-id": "a" }),
    ).toBeNull();
    expect(
      getRemoteParentFromLabels({ "ait.parent.server-id": "s", "ait.parent.agent-id": 7 }),
    ).toBeNull();
    expect(getRemoteParentFromLabels(null)).toBeNull();
    expect(getRemoteParentFromLabels(undefined)).toBeNull();
  });

  test("a remote parent does not read as a local parent", () => {
    const labels = { "ait.parent.server-id": "s", "ait.parent.agent-id": "a" };
    expect(getParentAgentIdFromLabels(labels)).toBeNull();
  });

  test("recognizes only client-scoped open-tab labels", () => {
    expect(isOpenAgentTabLabel(getOpenAgentTabLabel("client-a"))).toBe(true);
    expect(isOpenAgentTabLabel("paseo.open-agent-tab")).toBe(false);
    expect(isOpenAgentTabLabel("custom.open-agent-tab.client-a")).toBe(false);
  });
});

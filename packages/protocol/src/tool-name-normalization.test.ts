import { describe, expect, it } from "vitest";

import { getAitToolLeafName, isAitToolName } from "./tool-name-normalization.js";

describe("Ait tool names", () => {
  it.each([
    ["mcp__ait__create_agent", "create_agent"],
    ["mcp__ait_remote__create_agent", "create_agent"],
    ["ait.create_agent", "create_agent"],
    ["ait_remote.create_agent", "create_agent"],
  ])("recognizes %s", (name, leaf) => {
    expect(isAitToolName(name)).toBe(true);
    expect(getAitToolLeafName(name)).toBe(leaf);
  });

  it.each(["mcp__paseo__create_agent", "paseo.create_agent", "paseo_remote.create_agent"])(
    "does not recognize the retired paseo namespace in %s",
    (name) => {
      expect(isAitToolName(name)).toBe(false);
      expect(getAitToolLeafName(name)).toBeNull();
    },
  );

  it("does not treat speak as an Ait tool", () => {
    expect(isAitToolName("mcp__ait__speak")).toBe(false);
  });
});

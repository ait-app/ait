import { describe, expect, it } from "vitest";
import { getBuiltInLaunchOrder } from "./catalog";

describe("getBuiltInLaunchOrder", () => {
  it("leads with creating work in a primary pane", () => {
    expect(getBuiltInLaunchOrder("primary")).toEqual([
      "agent",
      "bot",
      "terminal",
      "changes",
      "diff",
      "files",
      "browser",
      "pullRequest",
      "schedule",
    ]);
  });

  it("leads with companion tools in a supporting pane", () => {
    expect(getBuiltInLaunchOrder("supporting")).toEqual([
      "changes",
      "terminal",
      "diff",
      "files",
      "agent",
      "bot",
      "browser",
      "pullRequest",
      "schedule",
    ]);
  });
});

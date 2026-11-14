import { describe, expect, it } from "vitest";
import { normalizeBranchOptionName } from "./branch-suggestions";

describe("normalizeBranchOptionName", () => {
  it("normalizes local and origin-prefixed refs", () => {
    expect(normalizeBranchOptionName("refs/heads/main")).toBe("main");
    expect(normalizeBranchOptionName("refs/remotes/origin/main")).toBe("main");
    expect(normalizeBranchOptionName("origin/feature/test")).toBe("feature/test");
    expect(normalizeBranchOptionName("feature/test")).toBe("feature/test");
  });

  it("filters out empty values and HEAD", () => {
    expect(normalizeBranchOptionName("")).toBeNull();
    expect(normalizeBranchOptionName("   ")).toBeNull();
    expect(normalizeBranchOptionName("HEAD")).toBeNull();
    expect(normalizeBranchOptionName("origin/HEAD")).toBeNull();
  });
});

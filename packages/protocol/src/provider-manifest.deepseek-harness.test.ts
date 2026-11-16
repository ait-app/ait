import { describe, expect, it } from "vitest";
import { BUILTIN_PROVIDER_IDS, getAgentProviderDefinition } from "./provider-manifest.js";

describe("DeepSeek Harness provider", () => {
  it("has a built-in definition without invented permission modes", () => {
    expect(BUILTIN_PROVIDER_IDS).toContain("deepseek-harness");
    expect(getAgentProviderDefinition("deepseek-harness")).toMatchObject({
      label: "DeepSeek Harness",
      defaultModeId: null,
      modes: [],
    });
  });
});

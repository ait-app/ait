import { describe, expect, it } from "vitest";
import { BUILTIN_PROVIDER_IDS, getAgentProviderDefinition } from "./provider-manifest.js";
import { ProviderOverridesSchema } from "./provider-config.js";

describe("MiniMax Code ACP provider", () => {
  it("registers the native provider with dynamically discovered modes", () => {
    expect(BUILTIN_PROVIDER_IDS).toContain("minimax-code");
    expect(getAgentProviderDefinition("minimax-code")).toMatchObject({
      label: "MiniMax Code",
      defaultModeId: null,
      modes: [],
    });
    expect(ProviderOverridesSchema.safeParse({ "minimax-code": { enabled: true } }).success).toBe(
      true,
    );
  });
});

import { describe, expect, it } from "vitest";
import {
  BUILTIN_PROVIDER_IDS,
  getAgentProviderDefinition,
  getUnattendedModeId,
} from "./provider-manifest.js";
import { ProviderOverridesSchema } from "./provider-config.js";

describe("Antigravity provider", () => {
  it("registers the native provider with local permissions as its default", () => {
    expect(BUILTIN_PROVIDER_IDS).toContain("antigravity");
    expect(getAgentProviderDefinition("antigravity")).toMatchObject({
      label: "Antigravity",
      defaultModeId: "default",
    });
    expect(getUnattendedModeId("antigravity")).toBe("full-access");
    expect(ProviderOverridesSchema.safeParse({ antigravity: { enabled: true } }).success).toBe(
      true,
    );
    expect(
      ProviderOverridesSchema.safeParse({
        "custom-agy": { extends: "antigravity", label: "Custom AGY" },
      }).success,
    ).toBe(true);
  });
});

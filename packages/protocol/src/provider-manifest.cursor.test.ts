import { describe, expect, it } from "vitest";
import {
  BUILTIN_PROVIDER_IDS,
  getAgentProviderDefinition,
  getUnattendedModeId,
} from "./provider-manifest.js";
import { ProviderOverridesSchema } from "./provider-config.js";

describe("Cursor provider", () => {
  it("exposes native modes and keeps the runtime default", () => {
    expect(BUILTIN_PROVIDER_IDS).toContain("cursor");
    const definition = getAgentProviderDefinition("cursor");
    expect(definition.label).toBe("Cursor");
    expect(definition.defaultModeId).toBeNull();
    expect(definition.modes.map((mode) => mode.id)).toEqual(["agent", "plan", "ask"]);
    expect(getUnattendedModeId("cursor")).toBeUndefined();
    expect(ProviderOverridesSchema.safeParse({ cursor: { enabled: true } }).success).toBe(true);
    expect(
      ProviderOverridesSchema.safeParse({
        "custom-cursor": { extends: "cursor", label: "Custom Cursor" },
      }).success,
    ).toBe(true);
  });
});

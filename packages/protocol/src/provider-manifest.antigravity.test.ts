import { describe, expect, it } from "vitest";
import {
  BUILTIN_PROVIDER_IDS,
  getAgentProviderDefinition,
  getUnattendedModeId,
} from "./provider-manifest.js";

describe("Antigravity provider", () => {
  it("registers the native provider with local permissions as its default", () => {
    expect(BUILTIN_PROVIDER_IDS).toContain("antigravity");
    expect(getAgentProviderDefinition("antigravity")).toMatchObject({
      label: "Antigravity",
      defaultModeId: "default",
    });
    expect(getUnattendedModeId("antigravity")).toBe("full-access");
  });
});

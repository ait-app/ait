import { describe, expect, it } from "vitest";
import {
  BUILTIN_PROVIDER_IDS,
  getAgentProviderDefinition,
  getUnattendedModeId,
} from "./provider-manifest.js";

describe("Pi provider", () => {
  it("mirrors the native provider's default mode", () => {
    expect(BUILTIN_PROVIDER_IDS).toContain("pi");
    expect(getAgentProviderDefinition("pi")).toMatchObject({
      label: "Pi",
      defaultModeId: "default",
      modes: [
        {
          id: "default",
          label: "Full Access",
          icon: "ShieldOff",
          colorTier: "dangerous",
          isUnattended: true,
        },
      ],
    });
    expect(getUnattendedModeId("pi")).toBe("default");
  });
});

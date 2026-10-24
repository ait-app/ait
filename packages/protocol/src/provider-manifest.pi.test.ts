import { describe, expect, it } from "vitest";
import {
  BUILTIN_PROVIDER_IDS,
  getAgentProviderDefinition,
  getUnattendedModeId,
} from "./provider-manifest.js";

describe("Pi provider", () => {
  it("does not expose a host mode for native Pi tools", () => {
    expect(BUILTIN_PROVIDER_IDS).toContain("pi");
    expect(getAgentProviderDefinition("pi")).toMatchObject({
      label: "Pi",
      defaultModeId: null,
      modes: [],
    });
    expect(getUnattendedModeId("pi")).toBeUndefined();
  });
});

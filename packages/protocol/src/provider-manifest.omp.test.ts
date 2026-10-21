import { describe, expect, it } from "vitest";
import { getAgentProviderDefinition, getUnattendedModeId } from "./provider-manifest.js";
import { ProviderOverridesSchema } from "./provider-config.js";

describe("Oh My Pi provider", () => {
  it("is enabled by default and accepts user overrides", () => {
    const omp = getAgentProviderDefinition("omp");
    expect(omp.label).toBe("Oh My Pi");
    expect(omp.enabledByDefault).not.toBe(false);
    expect(
      ProviderOverridesSchema.safeParse({
        omp: { enabled: true },
      }).success,
    ).toBe(true);
  });

  it("describes native OMP permission scopes accurately", () => {
    const omp = getAgentProviderDefinition("omp");
    expect(omp.modes.find((mode) => mode.id === "write")).toMatchObject({
      label: "Allow File Writes",
      description: "Allows reads and file writes; command execution requires approval.",
    });
    expect(omp.modes.map((mode) => mode.id)).toEqual(["full", "write", "ask", "ask-cached"]);
    expect(getUnattendedModeId("omp")).toBe("full");
  });
});

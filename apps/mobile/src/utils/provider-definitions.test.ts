import { describe, expect, it } from "vitest";
import type { ProviderSnapshotEntry } from "@ait/protocol/agent-types";
import { buildProviderDefinitions } from "./provider-definitions";

function snapshot(provider: string): ProviderSnapshotEntry {
  return {
    provider,
    status: "ready",
    enabled: true,
    modes: [
      { id: "full", label: "Full Access" },
      { id: "write", label: "Allow File Writes" },
      { id: "ask", label: "Always Ask" },
      { id: "ask-cached", label: "Ask and Remember" },
    ],
  };
}

describe("native provider mode visuals", () => {
  it("uses known OMP mode icons when native RPC supplies labels without icons", () => {
    const [definition] = buildProviderDefinitions([snapshot("omp")]);
    expect(definition.modes.map((mode) => mode.icon)).toEqual([
      "ShieldOff",
      "ShieldAlert",
      "ShieldCheck",
      "ShieldCheck",
    ]);
    expect(definition.modes.map((mode) => mode.colorTier)).toEqual([
      "dangerous",
      "moderate",
      "safe",
      "safe",
    ]);
  });
  it("preserves explicit native visuals and never invents unadvertised modes", () => {
    const entry = snapshot("omp");
    entry.modes = [{ id: "full", label: "Custom", icon: "Hammer", colorTier: "planning" }];
    expect(buildProviderDefinitions([entry])[0].modes).toEqual(entry.modes);
  });
  it("keeps unknown provider fallbacks and empty snapshots", () => {
    expect(buildProviderDefinitions([snapshot("custom")])[0].modes[0].icon).toBe("ShieldCheck");
    expect(buildProviderDefinitions(undefined)).toEqual([]);
  });
});

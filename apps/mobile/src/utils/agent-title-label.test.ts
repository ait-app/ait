import { describe, expect, it } from "vitest";
import { resolveAgentTitleLabel } from "./agent-title-label";

describe("resolveAgentTitleLabel", () => {
  it("returns null when title is not a string", () => {
    expect(resolveAgentTitleLabel(null)).toBe(null);
    expect(resolveAgentTitleLabel(undefined)).toBe(null);
  });

  it("returns null for whitespace-only titles", () => {
    expect(resolveAgentTitleLabel("   ")).toBe(null);
  });

  it("returns null for the placeholder 'new agent' regardless of case", () => {
    expect(resolveAgentTitleLabel("new agent")).toBe(null);
    expect(resolveAgentTitleLabel("New Agent")).toBe(null);
    expect(resolveAgentTitleLabel("  NEW AGENT  ")).toBe(null);
  });

  it("returns the trimmed title for real names", () => {
    expect(resolveAgentTitleLabel("  Build the thing  ")).toBe("Build the thing");
  });
});

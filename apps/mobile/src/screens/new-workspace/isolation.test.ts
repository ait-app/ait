import { describe, expect, it } from "vitest";
import {
  availableIsolations,
  createsWorktree,
  parseIsolationOptionId,
  resolveEffectiveIsolation,
} from "./isolation";

const full = { canCreateWorktree: true, canSandboxWorktree: true };
const noSandbox = { canCreateWorktree: true, canSandboxWorktree: false };
const noWorktree = { canCreateWorktree: false, canSandboxWorktree: false };

describe("resolveEffectiveIsolation", () => {
  it("keeps a supported choice", () => {
    expect(resolveEffectiveIsolation("sandboxed-worktree", full)).toBe("sandboxed-worktree");
    expect(resolveEffectiveIsolation("worktree", full)).toBe("worktree");
    expect(resolveEffectiveIsolation("local", full)).toBe("local");
  });

  it("falls back from sandboxed to a plain worktree when the host cannot confine writes", () => {
    expect(resolveEffectiveIsolation("sandboxed-worktree", noSandbox)).toBe("worktree");
  });

  it("falls back to local when the project cannot host a worktree", () => {
    expect(resolveEffectiveIsolation("sandboxed-worktree", noWorktree)).toBe("local");
    expect(resolveEffectiveIsolation("worktree", noWorktree)).toBe("local");
  });
});

describe("availableIsolations", () => {
  it("offers the sandboxed level only on hosts that can confine writes", () => {
    expect(availableIsolations(full)).toEqual(["local", "worktree", "sandboxed-worktree"]);
    expect(availableIsolations(noSandbox)).toEqual(["local", "worktree"]);
    expect(availableIsolations(noWorktree)).toEqual(["local"]);
  });
});

describe("createsWorktree", () => {
  it("is true for both worktree levels", () => {
    expect(createsWorktree("worktree")).toBe(true);
    expect(createsWorktree("sandboxed-worktree")).toBe(true);
    expect(createsWorktree("local")).toBe(false);
  });
});

describe("parseIsolationOptionId", () => {
  it("round-trips known ids and treats unknown ids as local", () => {
    expect(parseIsolationOptionId("sandboxed-worktree")).toBe("sandboxed-worktree");
    expect(parseIsolationOptionId("worktree")).toBe("worktree");
    expect(parseIsolationOptionId("bogus")).toBe("local");
  });
});

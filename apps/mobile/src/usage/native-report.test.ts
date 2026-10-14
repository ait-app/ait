import { describe, expect, it } from "vitest";
import { toUsageReport } from "./native-report";

describe("native quota presentation", () => {
  it("preserves account identity, zero usage, window pins, and balances", () => {
    const entry = toUsageReport(
      {
        providerId: "codex",
        displayName: "Codex",
        accountLabel: "account@example.test",
        status: "available",
        planLabel: "Plus",
        windows: [{ id: "primary", label: "Session", shortLabel: "5h", summary: true, usedPct: 0 }],
        balances: [{ id: "credits", label: "Credits", remaining: 10, unit: "credits" }],
      },
      "2026-10-10T00:00:00Z",
    );
    expect(entry.account.label).toBe("account@example.test");
    expect(entry.report).toMatchObject({
      status: "available",
      windows: [{ usedPct: 0, summary: true }],
    });
  });

  it("keeps expired login remedies and safe errors separate from quota facts", () => {
    const provider = {
      providerId: "claude",
      displayName: "Claude",
      status: "unavailable" as const,
      planLabel: null,
      windows: [],
    };
    const problem = {
      kind: "expired" as const,
      expiresAt: "2026-10-01T00:00:00Z",
      refreshedBy: "claude /login",
    };
    expect(toUsageReport({ ...provider, problem }, "now").report).toEqual({
      status: "unavailable",
      problem,
    });
    expect(toUsageReport(provider, "now").report).toMatchObject({ problem: { kind: "no_quota" } });
    expect(
      toUsageReport({ ...provider, status: "error", error: "Native query failed" }, "now").report,
    ).toEqual({ status: "error", error: "Native query failed" });
  });
});

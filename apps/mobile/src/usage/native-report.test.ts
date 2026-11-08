import { describe, expect, it } from "vitest";
import { toUsageReport, toUsageReports } from "./native-report";

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

it("omits unavailable sources while retaining errors, zero quota, and zero balance", () => {
  const base = { displayName: "Provider", planLabel: null, windows: [] };
  const reports = toUsageReports(
    [
      { ...base, providerId: "missing", status: "unavailable" },
      { ...base, providerId: "failed", status: "error", error: "Timed out" },
      {
        ...base,
        providerId: "zero",
        status: "available",
        windows: [{ id: "primary", label: "Primary", usedPct: 0 }],
        balances: [{ id: "credit", label: "Credit", remaining: 0, unit: "credits" }],
      },
      {
        ...base,
        providerId: "full",
        status: "available",
        windows: [{ id: "primary", label: "Primary", usedPct: 100 }],
      },
    ],
    "now",
  );
  expect(reports.map((report) => report.id)).toEqual(["failed", "zero", "full"]);
  expect(reports[1].report).toMatchObject({
    windows: [{ usedPct: 0 }],
    balances: [{ remaining: 0 }],
  });
  expect(reports[2].report).toMatchObject({ windows: [{ usedPct: 100 }] });
  expect(
    toUsageReports([{ ...base, providerId: "missing", status: "unavailable" }], "now"),
  ).toEqual([]);
});

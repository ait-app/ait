import { describe, expect, it } from "vitest";
import {
  ProviderUsageListRequestMessageSchema,
  ProviderUsageSchema,
  UsageReportEntrySchema,
  ProviderUsageBalanceSchema,
} from "./messages.js";

describe("native usage report compatibility", () => {
  it("preserves DeepSeek CNY balances without inventing usage or a limit", () => {
    const balance = ProviderUsageBalanceSchema.parse({
      id: "total-cny",
      label: "DeepSeek total balance",
      remaining: 12.01,
      unit: "cny",
    });
    expect(balance.unit).toBe("cny");
    expect(balance.used).toBeUndefined();
    expect(balance.limit).toBeUndefined();
  });
  it("accepts legacy host requests and session-scoped forced refresh", () => {
    expect(
      ProviderUsageListRequestMessageSchema.parse({
        type: "provider.usage.list.request",
        requestId: "old",
      }),
    ).toEqual({ type: "provider.usage.list.request", requestId: "old" });
    expect(
      ProviderUsageListRequestMessageSchema.parse({
        type: "provider.usage.list.request",
        requestId: "new",
        agentId: "agent",
        providerId: "codex",
        forceRefresh: true,
      }),
    ).toMatchObject({ agentId: "agent", providerId: "codex", forceRefresh: true });
    expect(
      ProviderUsageListRequestMessageSchema.safeParse({
        type: "provider.usage.list.request",
        requestId: "bad",
        forceRefresh: "yes",
      }).success,
    ).toBe(false);
  });
  it("preserves safe account problems and summary window metadata", () => {
    const provider = ProviderUsageSchema.parse({
      providerId: "claude",
      displayName: "Claude",
      status: "unavailable",
      planLabel: null,
      windows: [],
      problem: { kind: "rejected", status: 401, refreshedBy: "claude /login" },
    });
    expect(provider.problem).toMatchObject({ kind: "rejected", status: 401 });
    expect(
      UsageReportEntrySchema.parse({
        id: "codex",
        sourceId: "codex",
        sourceLabel: "Codex",
        account: {},
        fetchedAt: "now",
        report: {
          status: "available",
          windows: [
            { id: "primary", label: "Session", shortLabel: "5h", summary: true, usedPct: 0 },
          ],
        },
      }).report,
    ).toMatchObject({ windows: [{ summary: true, usedPct: 0 }] });
  });
});

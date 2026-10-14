import type { ProviderUsage, UsageReportEntry } from "@ait/protocol/messages";

/** Keep native quota facts and safe authentication failures intact for every usage surface. */
export function toUsageReport(provider: ProviderUsage, fetchedAt: string): UsageReportEntry {
  // Leave absent messages for the renderer, so cached reports follow language changes.
  const report: UsageReportEntry["report"] =
    provider.status === "available"
      ? {
          status: "available",
          ...(provider.planLabel ? { planLabel: provider.planLabel } : {}),
          windows: provider.windows,
          balances: provider.balances,
          details: provider.details,
        }
      : provider.status === "error"
        ? { status: "error", error: provider.error ?? "" }
        : {
            status: "unavailable",
            problem: provider.problem ?? {
              kind: "no_quota",
              detail: provider.error ?? "",
            },
          };
  return {
    id: provider.providerId,
    sourceId: provider.providerId,
    sourceLabel: provider.displayName,
    account: provider.accountLabel ? { label: provider.accountLabel } : {},
    fetchedAt: provider.fetchedAt ?? fetchedAt,
    report,
  };
}

import type { UsageProblem } from "@ait/protocol/messages";
import { useMemo } from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import { i18n } from "@/i18n/i18next";
import { formatCompactTimeAgo } from "@/utils/time";

export function formatUsageTimeAgo(
  label: string,
  date?: Date,
  t: TFunction = i18n.t.bind(i18n),
): string {
  if (label === "now") return t("providerUsage.justNow");
  const duration = /^(\d+)([mhd])$/.exec(label);
  if (duration) {
    const unit = duration[2] === "m" ? "minutes" : duration[2] === "h" ? "hours" : "days";
    const time = t(`providerUsage.${unit}`, { count: Number(duration[1]) });
    return t("providerUsage.ago", { time });
  }
  return date ? date.toLocaleDateString(i18n.language, { month: "short", day: "numeric" }) : label;
}

function createUsageCopy(t: TFunction) {
  return {
    problem: (problem: UsageProblem, now: Date = new Date()): string => {
      if (problem.kind === "no_quota")
        return problem.detail || t("providerUsage.nativePlanUnavailable");
      const remedy = problem.refreshedBy
        ? t("providerUsage.loginRefreshCommand", { command: problem.refreshedBy })
        : t("providerUsage.loginSignInAgain");
      if (problem.kind === "rejected")
        return t("providerUsage.loginRejected", { status: problem.status, remedy });
      const expiredAt = new Date(problem.expiresAt);
      const time = formatUsageTimeAgo(formatCompactTimeAgo(expiredAt, now), expiredAt, t);
      return t("providerUsage.loginExpired", { time, remedy });
    },
    get planUsage() {
      return t("providerUsage.title");
    },
    get options() {
      return t("providerUsage.options");
    },
    get refresh() {
      return t("providerUsage.refresh");
    },
    get refreshAll() {
      return t("providerUsage.refreshAll");
    },
    get refreshing() {
      return t("providerUsage.refreshing");
    },
    get refreshFailed() {
      return t("providerUsage.refreshFailed");
    },
    get loading() {
      return t("providerUsage.loading");
    },
    get empty() {
      return t("providerUsage.empty");
    },
    get noHosts() {
      return t("providerUsage.noHosts");
    },
    get errorTitle() {
      return t("providerUsage.errorTitle");
    },
    get error() {
      return t("providerUsage.error");
    },
    get unavailable() {
      return t("providerUsage.unavailable");
    },
    agentError: (reason: string) => t("providerUsage.agentError", { reason }),
    hostUnavailable: (host: string) => t("providerUsage.hostUnavailableNamed", { host }),
    hostUpgradeRequired: (host: string) => t("providerUsage.hostUpgradeRequiredNamed", { host }),
    get clientUnavailable() {
      return t("providerUsage.clientUnavailable");
    },
    get retry() {
      return t("providerUsage.retry");
    },
    get pin() {
      return t("providerUsage.pin");
    },
    get unpin() {
      return t("providerUsage.unpin");
    },
    pinWindow: (source: string, window: string) => t("providerUsage.pinWindow", { source, window }),
    refreshSource: (source: string) => t("providerUsage.refreshSource", { source }),
    get displayAs() {
      return t("providerUsage.displayAs");
    },
    get displayUsed() {
      return t("providerUsage.displayUsed");
    },
    get displayRemaining() {
      return t("providerUsage.displayRemaining");
    },
    get showInSidebar() {
      return t("providerUsage.showInSidebar");
    },
    get showInSidebarHint() {
      return t("providerUsage.showInSidebarHint");
    },
    get nativeReadFailed() {
      return t("providerUsage.nativeReadFailed");
    },
  } as const;
}

export type UsageCopy = ReturnType<typeof createUsageCopy>;
// Resolve copy when read, so non-React view models use the current language too.
export const usageCopy = createUsageCopy(i18n.t.bind(i18n));

/** Language changes also invalidate component and React Compiler memoization. */
export function useUsageCopy() {
  const { t } = useTranslation();
  return useMemo(() => createUsageCopy(t), [t]);
}

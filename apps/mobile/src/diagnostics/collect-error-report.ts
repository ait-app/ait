import { Platform } from "react-native";
import { getDesktopAppLogs, getDesktopDaemonLogs } from "@/desktop/daemon/desktop-daemon";
import { isElectronRuntime } from "@/desktop/host";
import { getHostRuntimeStore } from "@/runtime/host-runtime";
import { getStartupMarks } from "@/runtime/startup-timeline";
import { resolveAppVersion } from "@/utils/app-version";
import {
  formatAppDiagnosticHeader,
  formatHostRuntimeSection,
  formatServerInfoSection,
  formatStartupTimelineSection,
  redactAppDiagnosticReport,
} from "./app-diagnostic-report";
import { ERROR_REPORT_MAX_LENGTH, formatErrorReport, type AppErrorReport } from "./error-reporter";

const LOG_TIMEOUT_MS = 2000;
const collections = new WeakMap<AppErrorReport, Promise<string>>();

/** Only read existing local state and log tails; exporting a failure never sends new RPCs. */
export function collectErrorReport(report: AppErrorReport): Promise<string> {
  const existing = collections.get(report);
  if (existing) return existing;
  const collection = buildErrorReport(report);
  collections.set(report, collection);
  return collection;
}

async function buildErrorReport(report: AppErrorReport): Promise<string> {
  const sections = [formatErrorReport(report)];
  const store = getHostRuntimeStore();
  const hosts = store.getHosts();
  sections.push(
    formatAppDiagnosticHeader({
      appVersion: resolveAppVersion(),
      platform: Platform.OS,
      isDesktopApp: isElectronRuntime(),
      hostCount: hosts.length,
    }),
    formatStartupTimelineSection(getStartupMarks()),
  );
  for (const host of hosts) {
    const snapshot = store.getSnapshot(host.serverId);
    sections.push(formatHostRuntimeSection({ host, snapshot }));
    sections.push(formatServerInfoSection(snapshot?.client?.getLastServerInfoMessage() ?? null));
  }
  if (isElectronRuntime()) {
    const results = await Promise.all([
      readLogTail("Desktop daemon log tail", getDesktopDaemonLogs),
      readLogTail("Desktop app log tail", getDesktopAppLogs),
    ]);
    sections.push(...results);
  }
  const sanitized = redactAppDiagnosticReport(sections.join("\n\n"), hosts);
  return sanitized.length > ERROR_REPORT_MAX_LENGTH
    ? `${sanitized.slice(0, ERROR_REPORT_MAX_LENGTH)}\n[truncated]`
    : sanitized;
}

async function readLogTail(
  label: string,
  read: () => Promise<{ contents: string }>,
): Promise<string> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    const result = await Promise.race([
      read(),
      new Promise<never>((_resolve, reject) => {
        timer = setTimeout(() => reject(new Error("Log collection timed out")), LOG_TIMEOUT_MS);
      }),
    ]);
    return `${label}:\n${result.contents || "No log lines found"}`;
  } catch (error) {
    return `${label}:\nUnavailable: ${error instanceof Error ? error.message : String(error)}`;
  } finally {
    if (timer) clearTimeout(timer);
  }
}

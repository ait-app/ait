import { formatCaughtValue } from "@/components/root-error-details";
import { redactAppDiagnosticReport } from "./app-diagnostic-report";

export interface AppErrorReport {
  id: string;
  timestamp: string;
  source: string;
  message: string;
  details: string;
  context: string;
  logs: readonly string[];
}

interface ReportOptions {
  source: string;
  message?: string;
  context?: unknown;
  notify?: boolean;
}

const MAX_REPORTS = 50;
const MAX_LOGS = 80;
const MAX_DETAIL_LENGTH = 32_768;
export const ERROR_REPORT_MAX_LENGTH = 262_144;

function sanitize(value: unknown): string {
  const text = redactAppDiagnosticReport(
    formatCaughtValue(value).slice(0, MAX_DETAIL_LENGTH + 1),
    [],
  );
  return text.length > MAX_DETAIL_LENGTH
    ? `${text.slice(0, MAX_DETAIL_LENGTH)}\n[truncated]`
    : text;
}

/** Bounded, memory-only diagnostics. Raw exceptions and credentials are never retained. */
export function createErrorReporter() {
  let sequence = 0;
  const reports: AppErrorReport[] = [];
  const logs: string[] = [];
  const pending: AppErrorReport[] = [];
  const listeners = new Set<() => void>();
  const identities = new WeakMap<object, AppErrorReport>();
  const notified = new WeakSet<AppErrorReport>();
  let notificationScheduled = false;
  const notify = (entry: AppErrorReport, options: ReportOptions) => {
    if (options.notify === false || notified.has(entry)) return;
    notified.add(entry);
    pending.push(entry);
    if (pending.length > MAX_REPORTS) pending.shift();
    if (notificationScheduled) return;
    notificationScheduled = true;
    queueMicrotask(() => {
      notificationScheduled = false;
      for (const listener of listeners) listener();
    });
  };

  const report = (error: unknown, options: ReportOptions): AppErrorReport => {
    const message = sanitize(options.message ?? errorMessage(error));
    const known = error !== null && typeof error === "object" ? identities.get(error) : null;
    // Caches, console logging and UI handlers often report the same failure in one task.
    const recent = reports.findLast(
      (entry) => entry.message === message && Date.now() - Date.parse(entry.timestamp) < 2000,
    );
    const existing = known ?? recent;
    if (existing && reports.includes(existing)) {
      if (error instanceof Error) existing.details = sanitize(error);
      if (options.message) existing.message = message;
      if (error !== null && typeof error === "object") identities.set(error, existing);
      notify(existing, options);
      return existing;
    }

    const entry: AppErrorReport = {
      id: `error-${Date.now().toString(36)}-${++sequence}`,
      timestamp: new Date().toISOString(),
      source: options.source,
      message,
      details: sanitize(error),
      context: options.context === undefined ? "" : sanitize(options.context),
      logs: logs.slice(),
    };
    if (error !== null && typeof error === "object") identities.set(error, entry);
    reports.push(entry);
    if (reports.length > MAX_REPORTS) reports.shift();
    notify(entry, options);
    return entry;
  };

  return {
    report,
    presented(entry: AppErrorReport) {
      notified.add(entry);
      const index = pending.indexOf(entry);
      if (index !== -1) pending.splice(index, 1);
    },
    log(level: "warn" | "error", values: unknown[]) {
      logs.push(
        `${new Date().toISOString()} ${level.toUpperCase()} ${sanitize(values).slice(0, 8192)}`,
      );
      if (logs.length > MAX_LOGS) logs.shift();
    },
    getReports: (): readonly AppErrorReport[] => reports.slice(),
    takePending: (): AppErrorReport[] => pending.splice(0),
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

export const appErrorReporter = createErrorReporter();

export function errorMessage(error: unknown): string {
  try {
    if (error instanceof Error) return error.message || error.name || "Unknown error";
    if (
      error !== null &&
      typeof error === "object" &&
      "message" in error &&
      typeof error.message === "string"
    )
      return error.message;
    return typeof error === "string"
      ? error.trim()
        ? error
        : "Unknown error"
      : formatCaughtValue(error);
  } catch {
    return "Unknown error";
  }
}

export function formatErrorReport(report: AppErrorReport): string {
  const text = [
    "Ait error log",
    `Error ID: ${report.id}`,
    `Occurred at: ${report.timestamp}`,
    `Source: ${report.source}`,
    `Message: ${report.message}`,
    `Error chain and stack:\n${report.details}`,
    report.context ? `Context:\n${report.context}` : "",
    `Recent client warnings and errors (oldest first):\n${report.logs.join("\n") || "No preceding logs"}`,
  ]
    .filter(Boolean)
    .join("\n\n");
  return text.length > ERROR_REPORT_MAX_LENGTH
    ? `${text.slice(0, ERROR_REPORT_MAX_LENGTH)}\n[truncated]`
    : text;
}

/** Legacy error dialogs use the same cross-platform notification and copy controls. */
export function showErrorAlert(title: string, message: string, error: unknown = message): void {
  appErrorReporter.report(error, { source: "dialog", message: `${title}: ${message}` });
}

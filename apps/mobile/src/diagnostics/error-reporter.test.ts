import { afterEach, describe, expect, it, vi } from "vitest";
import { ERROR_REPORT_MAX_LENGTH, createErrorReporter, formatErrorReport } from "./error-reporter";

afterEach(() => vi.useRealTimers());

describe("error diagnostics", () => {
  it("keeps causes, aggregate failures, stack and request identities with preceding logs", () => {
    const reporter = createErrorReporter();
    reporter.log("warn", ["Connection re-established", { serverId: "host-1" }]);
    const cause = Object.assign(new Error("Disk full"), { code: "ENOSPC" });
    const failure = new AggregateError([cause], "Save failed", { cause });
    const report = reporter.report(failure, {
      source: "mutation",
      context: { requestId: "rpc-1", serverId: "host-1" },
    });
    const text = formatErrorReport(report);
    for (const detail of [
      "Save failed",
      "Disk full",
      "ENOSPC",
      "Cause:",
      "Errors:",
      "Stack:",
      "rpc-1",
      "host-1",
      "Connection re-established",
    ])
      expect(text).toContain(detail);
  });

  it("redacts secrets in messages, error chains, fields, URLs and breadcrumbs", () => {
    const reporter = createErrorReporter();
    const secret = "private-password";
    reporter.log("warn", [{ token: secret }, `Cookie: session=${secret}`]);
    const cause = Object.assign(new Error(`Bearer ${secret}`), { apiKey: secret });
    const report = reporter.report(
      new Error(`https://user:${secret}@host/?access_token=${secret}`, { cause }),
      {
        source: "test",
        context: { password: secret, ticket: secret },
      },
    );
    expect(formatErrorReport(report)).not.toContain(secret);
    expect(formatErrorReport(report)).toContain("[redacted]");
  });

  it("deduplicates repeated catch layers without suppressing notification of inline errors", async () => {
    const reporter = createErrorReporter();
    const error = new Error("Failed request");
    const listener = vi.fn();
    const unsubscribe = reporter.subscribe(listener);
    const first = reporter.report(error, { source: "inline", notify: false });
    expect(listener).not.toHaveBeenCalled();
    expect(reporter.report(error, { source: "query" })).toBe(first);
    expect(reporter.report(error.message, { source: "toast" })).toBe(first);
    await Promise.resolve();
    expect(listener).toHaveBeenCalledTimes(1);
    expect(reporter.takePending()).toEqual([first]);
    expect(reporter.takePending()).toEqual([]);
    unsubscribe();
  });

  it("batches console and UI handlers so the same error is only presented once", async () => {
    const reporter = createErrorReporter();
    const listener = vi.fn();
    reporter.subscribe(listener);
    const error = new Error("Underlying failure");
    const report = reporter.report(error, { source: "console.error" });
    reporter.report(error, { source: "toast", message: "Friendly failure message", notify: false });
    reporter.presented(report);
    await Promise.resolve();
    expect(report.message).toBe("Friendly failure message");
    expect(reporter.takePending()).toEqual([]);
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it("bounds history, pending errors, large details and preceding logs", () => {
    const reporter = createErrorReporter();
    for (let index = 0; index < 100; index++) {
      reporter.log("error", [`breadcrumb-${index}`]);
      reporter.report(`failure-${index}`, { source: "test" });
    }
    expect(reporter.getReports()).toHaveLength(50);
    expect(reporter.takePending()).toHaveLength(50);
    const report = reporter.report("x".repeat(100_000), { source: "test" });
    expect(report.details.length).toBeLessThan(33_000);
    expect(report.logs).toHaveLength(80);
    expect(report.logs[0]).toContain("breadcrumb-20");
    for (let index = 0; index < 80; index++) reporter.log("warn", ["x".repeat(10_000)]);
    const bounded = formatErrorReport(reporter.report("large log tail", { source: "test" }));
    expect(bounded.length).toBeLessThan(ERROR_REPORT_MAX_LENGTH + 20);
    expect(bounded).toContain("[truncated]");
  });

  it("handles malformed thrown values and records a new occurrence after the deduplication window", () => {
    vi.useFakeTimers();
    const reporter = createErrorReporter();
    const malformed = new Error("broken");
    Object.defineProperty(malformed, "message", {
      get() {
        throw new Error("bad getter");
      },
    });
    expect(() => reporter.report(malformed, { source: "test" })).not.toThrow();
    const first = reporter.report("offline", { source: "connection" });
    vi.advanceTimersByTime(2100);
    expect(reporter.report("offline", { source: "connection" }).id).not.toBe(first.id);
  });
});

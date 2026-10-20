import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { JSDOM } from "jsdom";
import { appErrorReporter } from "./error-reporter";
import { installErrorHandlers } from "./install-error-handlers";

let cleanup: (() => void) | undefined;

beforeEach(() => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  vi.spyOn(console, "warn").mockImplementation(() => {});
  appErrorReporter.takePending();
});
afterEach(() => {
  cleanup?.();
  cleanup = undefined;
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it("records console errors and SDK request diagnostics without a toast", () => {
  const original = console.error;
  cleanup = installErrorHandlers();
  console.warn("ordinary warning");
  expect(appErrorReporter.takePending()).toEqual([]);
  console.error(
    "Daemon request failed",
    JSON.stringify({
      err: { message: "RPC denied", stack: "at request" },
      requestId: "rpc-1",
      requestType: "workspace.get.request",
    }),
  );
  expect(appErrorReporter.takePending()).toEqual([]);
  const report = appErrorReporter.getReports().at(-1)!;
  expect(report.message).toBe("RPC denied");
  expect(report.context).toContain("rpc-1");
  expect(report.details).toContain("at request");
  expect(original).toHaveBeenCalledTimes(1);
  const failure = new Error("Save failed");
  console.error("Save failed", failure);
  expect(appErrorReporter.takePending()).toEqual([]);
  appErrorReporter.report(failure, { source: "dialog" });
  expect(appErrorReporter.takePending().map((pending) => pending.message)).toEqual(["Save failed"]);
  cleanup();
  expect(console.error).toBe(original);
});

it("captures unhandled browser errors and promise rejections and removes listeners", () => {
  const dom = new JSDOM();
  vi.stubGlobal("window", dom.window);
  cleanup = installErrorHandlers();
  const error = new Error("Unhandled browser failure");
  dom.window.dispatchEvent(new dom.window.ErrorEvent("error", { error, message: error.message }));
  const rejection = new dom.window.Event("unhandledrejection");
  Object.assign(rejection, { reason: new Error("Unhandled promise failure") });
  dom.window.dispatchEvent(rejection);
  expect(appErrorReporter.takePending()).toEqual([]);
  expect(
    appErrorReporter
      .getReports()
      .slice(-2)
      .map((report) => report.source),
  ).toEqual(["uncaught exception", "unhandled promise rejection"]);
  cleanup();
  dom.window.dispatchEvent(new dom.window.ErrorEvent("error", { message: "After cleanup" }));
  expect(appErrorReporter.takePending()).toEqual([]);
  dom.window.close();
});

it("chains the native fatal handler without swallowing exceptions", () => {
  const previous = vi.fn();
  let handler = previous;
  vi.stubGlobal("ErrorUtils", {
    getGlobalHandler: () => handler,
    setGlobalHandler: (next: typeof handler) => {
      handler = next;
    },
  });
  cleanup = installErrorHandlers();
  const error = new Error("Native fatal failure");
  handler(error, true);
  expect(previous).toHaveBeenCalledWith(error, true);
  expect(appErrorReporter.takePending()).toEqual([]);
  expect(appErrorReporter.getReports().at(-1)!.source).toBe("native exception");
  cleanup();
  expect(handler).toBe(previous);
});

it.each([undefined, {}, { addEventListener: true }, { addEventListener: vi.fn() }])(
  "installs and removes native diagnostics with a non-browser window: %j",
  (nativeWindow) => {
    vi.stubGlobal("window", nativeWindow);
    const previous = vi.fn();
    let handler = previous;
    vi.stubGlobal("ErrorUtils", {
      getGlobalHandler: () => handler,
      setGlobalHandler: (next: typeof handler) => {
        handler = next;
      },
    });
    const originalError = console.error;
    cleanup = installErrorHandlers();
    console.error(new Error("Native startup fixture"));
    handler(new Error("Native runtime fixture"), true);
    expect(previous).toHaveBeenCalledOnce();
    expect(appErrorReporter.getReports().at(-1)!.message).toBe("Native runtime fixture");
    expect(appErrorReporter.takePending()).toEqual([]);
    cleanup();
    expect(handler).toBe(previous);
    expect(console.error).toBe(originalError);
  },
);

it("preserves the native fatal handler when diagnostic collection fails", () => {
  const previous = vi.fn();
  let handler = previous;
  vi.stubGlobal("window", {});
  vi.stubGlobal("ErrorUtils", {
    getGlobalHandler: () => handler,
    setGlobalHandler: (next: typeof handler) => {
      handler = next;
    },
  });
  cleanup = installErrorHandlers();
  vi.spyOn(appErrorReporter, "report").mockImplementation(() => {
    throw new Error("Diagnostic collector unavailable");
  });
  const error = new Error("Original native failure");
  expect(() => handler(error, true)).not.toThrow();
  expect(previous).toHaveBeenCalledWith(error, true);
});

it("removes listeners from the original browser target even if window changes", () => {
  const dom = new JSDOM();
  vi.stubGlobal("window", dom.window);
  cleanup = installErrorHandlers();
  vi.stubGlobal("window", {});
  cleanup();
  const reportCount = appErrorReporter.getReports().length;
  dom.window.dispatchEvent(new dom.window.ErrorEvent("error", { message: "Detached target" }));
  expect(appErrorReporter.getReports()).toHaveLength(reportCount);
  dom.window.close();
});

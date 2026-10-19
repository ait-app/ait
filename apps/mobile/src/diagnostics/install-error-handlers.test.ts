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

it("captures console errors and SDK request diagnostics while preserving original handlers", () => {
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
  const [report] = appErrorReporter.takePending();
  expect(report.message).toBe("RPC denied");
  expect(report.context).toContain("rpc-1");
  expect(report.details).toContain("at request");
  expect(original).toHaveBeenCalledTimes(1);
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
  expect(appErrorReporter.takePending().map((report) => report.source)).toEqual([
    "uncaught exception",
    "unhandled promise rejection",
  ]);
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
  expect(appErrorReporter.takePending()[0].source).toBe("native exception");
  cleanup();
  expect(handler).toBe(previous);
});

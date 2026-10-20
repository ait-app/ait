import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { JSDOM } from "jsdom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider, useToast } from "@/contexts/toast-context";
import { ToastViewport, useToastHost, type ToastApi, type ToastState } from "./toast-host";
import { CopyErrorLogButton } from "./copy-error-log-button";
import { RootErrorBoundary, RootErrorFallback } from "./root-error-boundary";
import { Alert } from "./ui/alert";
import { appErrorReporter, formatErrorReport } from "@/diagnostics/error-reporter";

const { clipboard, exportReport, collectReport } = vi.hoisted(() => ({
  clipboard: vi.fn(),
  exportReport: vi.fn(),
  collectReport: vi.fn(),
}));
vi.mock("@/diagnostics/copy-error-log", () => ({ copyErrorLog: clipboard }));
vi.mock("@/diagnostics/export-diagnostic-report", () => ({ exportDiagnosticReport: exportReport }));
vi.mock("@/diagnostics/collect-error-report", () => ({ collectErrorReport: collectReport }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("react-native-reanimated", async () => {
  const { View } = await import("react-native");
  return {
    default: { View },
    FadeIn: { duration: () => undefined },
    FadeOut: { duration: () => undefined },
  };
});
vi.mock("@/constants/layout", () => ({
  useIsCompactFormFactor: () => false,
  HEADER_INNER_HEIGHT: 44,
  HEADER_INNER_HEIGHT_MOBILE: 44,
  HEADER_TOP_PADDING_MOBILE: 0,
}));

let root: Root;
let dom: JSDOM;
let api: ToastApi;
let currentToast: ToastState | null = null;

function ToastHarness() {
  api = useToast();
  return null;
}

function LocalToastHarness() {
  const host = useToastHost();
  api = host.api;
  currentToast = host.toast;
  return <ToastViewport toast={host.toast} onDismiss={host.dismiss} />;
}

beforeEach(() => {
  dom = new JSDOM("<!doctype html><html><body></body></html>");
  vi.stubGlobal("React", React);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("window", dom.window);
  vi.stubGlobal("document", dom.window.document);
  vi.stubGlobal("HTMLElement", dom.window.HTMLElement);
  vi.stubGlobal("Node", dom.window.Node);
  vi.stubGlobal("navigator", dom.window.navigator);
  const container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  clipboard.mockResolvedValue(undefined);
  exportReport.mockResolvedValue(undefined);
  collectReport.mockImplementation(
    async (report) => `${formatErrorReport(report)}\nDaemon log: request-42`,
  );
  appErrorReporter.takePending();
});

afterEach(async () => {
  await act(async () => root.unmount());
  appErrorReporter.takePending();
  vi.resetAllMocks();
  vi.unstubAllGlobals();
  dom.window.close();
});

const find = (id: string) => document.querySelector(`[data-testid="${id}"]`) as HTMLElement;
const click = async (id: string) =>
  act(async () => {
    find(id).click();
  });

describe("error notifications", () => {
  it("still offers error logs when an exception has an empty message", async () => {
    await act(async () => root.render(<LocalToastHarness />));
    await act(async () => api.error("", new Error("")));
    expect(find("app-toast-message").textContent).toBe("common.errors.error");
    expect(find("copy-error-log")).not.toBeNull();
  });
  it("presents a logged failure once when a panel also handles it", async () => {
    await act(async () =>
      root.render(
        <ToastProvider>
          <LocalToastHarness />
        </ToastProvider>,
      ),
    );
    const error = new Error("Logged underlying failure");
    await act(async () => {
      appErrorReporter.report(error, { source: "console.error" });
      api.error("Friendly panel failure", error);
    });
    expect(document.querySelectorAll('[data-testid="app-toast-message"]')).toHaveLength(1);
    expect(find("app-toast-message").textContent).toBe("Friendly panel failure");
    await click("dismiss-error-toast");
    expect(document.querySelectorAll('[data-testid="app-toast-message"]')).toHaveLength(0);
  });

  it("keeps crash details available when diagnostic collection and file export fail", async () => {
    collectReport.mockRejectedValue(new Error("Runtime unavailable"));
    exportReport.mockRejectedValue(new Error("File sharing unavailable"));
    await act(async () =>
      root.render(
        <CopyErrorLogButton
          error={new Error("Original crash", { cause: new Error("Crash cause") })}
        />,
      ),
    );
    await click("copy-error-log");
    expect(clipboard).toHaveBeenCalledWith(expect.stringContaining("Crash cause"));
    await click("export-error-log");
    expect(document.body.textContent).toContain("errorLog.exportFailed");
    expect(appErrorReporter.takePending()).toEqual([]);
  });

  it("keeps errors until dismissed and queues failures without success messages hiding them", async () => {
    await act(async () => root.render(<LocalToastHarness />));
    await act(async () => {
      api.show("First failure", {
        variant: "error",
        durationMs: 1,
        nativeAndroid: true,
        error: new Error("Original failure"),
      });
      api.error("Second failure");
      api.copied();
    });
    expect(currentToast?.durationMs).toBeNull();
    expect(find("app-toast-message").textContent).toBe("First failure");
    expect(find("copy-error-log")).not.toBeNull();
    await click("dismiss-error-toast");
    expect(find("app-toast-message").textContent).toBe("Second failure");
  });

  it("drains failures reported before mount and copies the full original cause and collected logs", async () => {
    const error = Object.assign(new Error("Operation failed", { cause: new Error("Disk full") }), {
      requestId: "rpc-1",
    });
    appErrorReporter.report(error, { source: "query" });
    await act(async () =>
      root.render(
        <ToastProvider>
          <ToastHarness />
        </ToastProvider>,
      ),
    );
    expect(find("app-toast-message").textContent).toBe("Operation failed");
    await click("copy-error-log");
    const copied = clipboard.mock.calls[0][0];
    for (const detail of [
      "Operation failed",
      "Disk full",
      "Cause:",
      "Stack:",
      "rpc-1",
      "request-42",
    ])
      expect(copied).toContain(detail);
    expect(find("copy-error-log").textContent).toBe("errorLog.copied");
  });

  it("keeps the original log and offers file export when clipboard access fails", async () => {
    clipboard.mockRejectedValue(new Error("Clipboard permission denied"));
    await act(async () =>
      root.render(<CopyErrorLogButton error={new Error("Export this failure")} />),
    );
    await click("copy-error-log");
    expect(document.body.textContent).toContain("errorLog.copyFailed");
    await click("export-error-log");
    expect(exportReport).toHaveBeenCalledWith(expect.stringContaining("Export this failure"));
    expect(appErrorReporter.takePending()).toEqual([]);
  });

  it("copies inline and fatal errors outside the app's toast provider", async () => {
    await act(async () =>
      root.render(
        <Alert
          variant="error"
          title="Read failed"
          error={new Error("Original inline exception")}
        />,
      ),
    );
    await click("copy-error-log");
    expect(clipboard).toHaveBeenLastCalledWith(
      expect.stringContaining("Original inline exception"),
    );
    await act(async () =>
      root.render(<RootErrorFallback error="Fatal failure" onReload={vi.fn()} />),
    );
    await click("copy-error-log");
    expect(clipboard).toHaveBeenLastCalledWith(expect.stringContaining("Fatal failure"));
  });

  it("preserves the fatal exception and React component stack in copied logs", async () => {
    const error = new Error("Render exploded", { cause: new Error("Render cause") });
    function BrokenRoute(): React.ReactNode {
      throw error;
    }
    const originalError = console.error;
    console.error = vi.fn();
    try {
      await act(async () =>
        root.render(
          <RootErrorBoundary onReload={vi.fn()}>
            <BrokenRoute />
          </RootErrorBoundary>,
        ),
      );
      await click("copy-error-log");
      expect(clipboard).toHaveBeenLastCalledWith(expect.stringContaining("componentStack"));
      expect(clipboard).toHaveBeenLastCalledWith(expect.stringContaining("Render cause"));
    } finally {
      console.error = originalError;
    }
  });
});

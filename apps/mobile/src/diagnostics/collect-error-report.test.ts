import { afterEach, expect, it, vi } from "vitest";
import { collectErrorReport } from "./collect-error-report";
import { createErrorReporter } from "./error-reporter";

const { sources } = vi.hoisted(() => ({
  sources: {
    daemon: vi.fn(),
    app: vi.fn(),
    desktop: true,
  },
}));
vi.mock("@/desktop/host", () => ({ isElectronRuntime: () => sources.desktop }));
vi.mock("@/desktop/daemon/desktop-daemon", () => ({
  getDesktopAppLogs: sources.app,
  getDesktopDaemonLogs: sources.daemon,
}));
vi.mock("@/utils/app-version", () => ({ resolveAppVersion: () => "1.2.3" }));
vi.mock("@/runtime/startup-timeline", () => ({ getStartupMarks: () => [] }));
vi.mock("@/runtime/host-runtime", () => ({
  getHostRuntimeStore: () => ({
    getHosts: () => [
      {
        serverId: "host-1",
        label: "Host",
        connections: [
          {
            type: "directTcp",
            id: "conn-1",
            endpoint: "ws://private-host",
            password: "private-password",
          },
        ],
      },
    ],
    getSnapshot: () => null,
  }),
}));
afterEach(() => {
  vi.useRealTimers();
  vi.resetAllMocks();
  sources.desktop = true;
});

it("adds and sanitizes desktop log tails, version and host state once per report", async () => {
  sources.daemon.mockResolvedValue({ contents: "requestId=rpc-1 private-password" });
  sources.app.mockResolvedValue({ contents: "app error api_key=private-key" });
  const report = createErrorReporter().report("Request failed", { source: "test" });
  const first = collectErrorReport(report);
  expect(collectErrorReport(report)).toBe(first);
  const text = await first;
  for (const value of [
    "Request failed",
    "rpc-1",
    "1.2.3",
    "Host",
    "Desktop daemon log tail",
    "Desktop app log tail",
  ])
    expect(text).toContain(value);
  for (const value of ["private-password", "private-key", "ws://private-host"])
    expect(text).not.toContain(value);
  expect(sources.daemon).toHaveBeenCalledTimes(1);
});

it("keeps the original report when log readers fail or hang", async () => {
  vi.useFakeTimers();
  sources.daemon.mockRejectedValue(new Error("Log unavailable"));
  sources.app.mockReturnValue(new Promise(() => {}));
  const result = collectErrorReport(
    createErrorReporter().report("Original failure", { source: "test" }),
  );
  await vi.advanceTimersByTimeAsync(2100);
  const text = await result;
  expect(text).toContain("Original failure");
  expect(text).toContain("Log unavailable");
  expect(text).toContain("Log collection timed out");
});

it("collects browser/mobile reports without desktop IPC", async () => {
  sources.desktop = false;
  const text = await collectErrorReport(
    createErrorReporter().report("Browser failure", { source: "test" }),
  );
  expect(text).toContain("Browser failure");
  expect(sources.app).not.toHaveBeenCalled();
});

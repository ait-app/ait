import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { ipcMain } from "electron";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { DEFAULT_DESKTOP_SETTINGS } from "../settings/desktop-settings";
import { createDaemonCommandHandlers, registerDaemonManager } from "./daemon-manager";

const mocks = vi.hoisted(() => ({
  paseoHome: "",
  start: vi.fn(),
  stop: vi.fn(),
  restart: vi.fn(),
  status: vi.fn(() => ({ listen: "127.0.0.1:43210", ownedByDesktop: true })),
  authorization: vi.fn(),
  openTransport: vi.fn(),
  settings: {
    releaseChannel: "stable",
    daemon: {
      manageBuiltInDaemon: true,
      keepRunningAfterQuit: true,
    },
  },
  createNodeEntrypointInvocation: vi.fn(() => ({
    command: "node",
    args: [],
    env: {},
  })),
  spawnProcess: vi.fn(),
  logInfo: vi.fn(),
  logError: vi.fn(),
  appLogPath: "",
  getElectronLogFile: vi.fn(),
  accountCommand: vi.fn(),
}));

vi.mock("electron", () => ({
  app: {
    getPath: vi.fn(() => mocks.paseoHome),
    getVersion: vi.fn(() => "1.2.3"),
    isPackaged: false,
  },
  ipcMain: { handle: vi.fn() },
  powerMonitor: { getSystemIdleTime: vi.fn(() => 0) },
}));

vi.mock("electron-log/main", () => ({
  default: {
    info: mocks.logInfo,
    error: mocks.logError,
    transports: {
      file: {
        getFile: mocks.getElectronLogFile,
      },
    },
  },
}));

vi.mock("./rust-daemon.js", () => ({
  resolveDesktopDaemonHome: () => mocks.paseoHome,
  RustDaemonManager: class {
    start = mocks.start;
    stop = mocks.stop;
    restart = mocks.restart;
    status = mocks.status;
    authorization = mocks.authorization;
  },
}));
vi.mock("./local-transport.js", () => ({
  openLocalTransportSession: mocks.openTransport,
  sendLocalTransportMessage: vi.fn(),
  closeLocalTransportSession: vi.fn(),
}));

vi.mock("./account-ipc.js", () => ({
  createAccountIpc: () => mocks.accountCommand,
}));

vi.mock("../settings/desktop-settings-electron.js", () => ({
  getDesktopSettingsStore: () => ({
    get: async () => mocks.settings,
    patch: vi.fn(),
    migrateLegacyRendererSettings: vi.fn(),
  }),
}));

vi.mock("./runtime-paths.js", () => ({
  createNodeEntrypointInvocation: mocks.createNodeEntrypointInvocation,
  resolveDaemonRunnerEntrypoint: vi.fn(() => ({
    entryPath: path.join(mocks.paseoHome, "daemon.js"),
    execArgv: [],
  })),
}));

describe("daemon-manager commands", () => {
  let fixtureRoot: string;

  beforeEach(() => {
    fixtureRoot = mkdtempSync(path.join(tmpdir(), "ait daemon manager "));
    mocks.paseoHome = path.join(fixtureRoot, "home");
    mocks.appLogPath = path.join(fixtureRoot, "main.log");
    mocks.settings = DEFAULT_DESKTOP_SETTINGS;
    mocks.createNodeEntrypointInvocation.mockReset();
    mocks.createNodeEntrypointInvocation.mockReturnValue({
      command: "node",
      args: [],
      env: {},
    });
    mocks.spawnProcess.mockReset();
    mocks.logInfo.mockReset();
    mocks.logError.mockReset();
    mocks.getElectronLogFile.mockReset();
    mocks.getElectronLogFile.mockReturnValue({ path: mocks.appLogPath });
    mocks.accountCommand.mockReset();
    vi.mocked(ipcMain.handle).mockReset();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    rmSync(fixtureRoot, { recursive: true, force: true });
  });

  it("routes host synchronization from the Ait bridge to the account authority", async () => {
    registerDaemonManager(() => true);
    const registration = vi
      .mocked(ipcMain.handle)
      .mock.calls.find(([channel]) => channel === "ait:invoke");
    expect(registration).toBeDefined();
    const event = {} as Electron.IpcMainInvokeEvent;
    const args = { serverId: "host", instanceId: "instance" };
    mocks.accountCommand.mockResolvedValue({ node_session_id: "lease" });
    await expect(registration![1](event, "account_host_sync", args)).resolves.toEqual({
      node_session_id: "lease",
    });
    expect(mocks.accountCommand).toHaveBeenCalledExactlyOnceWith(event, "account_host_sync", args);
  });

  it("rejects direct file downloads from a window outside the Ait app", async () => {
    registerDaemonManager(() => false);
    const registration = vi
      .mocked(ipcMain.handle)
      .mock.calls.find(([channel]) => channel === "ait:invoke")!;
    await expect(registration[1]({ sender: {} }, "file_download", {})).rejects.toThrow(
      "Invalid download window.",
    );
  });

  it("starts a direct download without waiting for the login shell environment", async () => {
    const environment = Promise.withResolvers<void>();
    registerDaemonManager(() => true, environment.promise);
    const registration = vi
      .mocked(ipcMain.handle)
      .mock.calls.find(([channel]) => channel === "ait:invoke")!;
    const sender = {} as Electron.WebContents;
    // The renderer already holds a one-use, 60-second download token, and the transfer is a
    // plain HTTP request through Electron, so the login shell lookup must not hold it back.
    // Only the daemon commands below need the environment.
    await expect(registration[1]({ sender }, "file_download", {})).rejects.toThrow(
      "Invalid download URL.",
    );
    mocks.status.mockClear();
    const pending = registration[1]({ sender }, "desktop_daemon_status");
    await Promise.resolve();
    expect(mocks.status).not.toHaveBeenCalled();
    environment.resolve();
    await expect(pending).resolves.toEqual(mocks.status());
    expect(mocks.status).toHaveBeenCalled();
  });

  it("binds local transport sessions to the requesting window", async () => {
    registerDaemonManager(() => true);
    const registration = vi
      .mocked(ipcMain.handle)
      .mock.calls.find(([channel]) => channel === "ait:invoke");
    const sender = {} as Electron.WebContents;
    const args = {
      sessionId: "owned",
      target: { transportType: "socket", transportPath: "/tmp/ait.sock" },
    };
    await registration![1](
      { sender } as Electron.IpcMainInvokeEvent,
      "open_local_daemon_transport",
      args,
    );
    expect(mocks.openTransport).toHaveBeenLastCalledWith(args, sender);
  });

  it("starts the Rust owner and injects managed credentials only in the main process", async () => {
    const handlers = createDaemonCommandHandlers();
    mocks.start.mockResolvedValue({ status: "running" });
    await expect(handlers.start_desktop_daemon()).resolves.toEqual({ status: "running" });
    mocks.authorization.mockReturnValue("private-token");
    await handlers.open_local_daemon_transport({
      sessionId: "test",
      target: { transportType: "rustTcp", url: "ws://localhost:43210/v1/ws" },
    });
    expect(mocks.openTransport).toHaveBeenLastCalledWith({
      sessionId: "test",
      bearerToken: "private-token",
      target: { transportType: "rustTcp", url: "ws://127.0.0.1:43210/v1/ws" },
    });
    mocks.authorization.mockReturnValue(undefined);
    const external = {
      sessionId: "external",
      target: { transportType: "rustTcp", url: "ws://localhost:43211/v1/ws" },
      bearerToken: "external-token",
    };
    await handlers.open_local_daemon_transport(external);
    expect(mocks.openTransport).toHaveBeenLastCalledWith(external);
  });

  it("honors the built-in management switch", async () => {
    mocks.settings = {
      ...DEFAULT_DESKTOP_SETTINGS,
      daemon: { manageBuiltInDaemon: false, keepRunningAfterQuit: false },
    };
    await expect(createDaemonCommandHandlers().start_desktop_daemon()).rejects.toThrow("disabled");
  });

  it("rejects starting or restarting a built-in daemon on Windows even with old settings", async () => {
    vi.stubGlobal("process", { ...process, platform: "win32" });
    mocks.start.mockClear();
    mocks.restart.mockClear();
    const handlers = createDaemonCommandHandlers();
    await expect(handlers.start_desktop_daemon()).rejects.toThrow("does not include");
    await expect(handlers.restart_desktop_daemon()).rejects.toThrow("does not include");
    expect(mocks.start).not.toHaveBeenCalled();
    expect(mocks.restart).not.toHaveBeenCalled();
  });

  it("returns the Electron main-process log tail from electron-log", () => {
    writeFileSync(
      mocks.appLogPath,
      Array.from({ length: 105 }, (_value, index) => `main log line ${index + 1}`).join("\n"),
    );
    const handlers = createDaemonCommandHandlers();

    expect(handlers.desktop_app_logs()).toEqual({
      logPath: mocks.appLogPath,
      contents: Array.from({ length: 100 }, (_value, index) => `main log line ${index + 6}`).join(
        "\n",
      ),
    });
  });

  it("exposes updater diagnostics through the desktop command boundary", () => {
    const diagnostics = createDaemonCommandHandlers().desktop_update_diagnostics();

    expect(diagnostics).toMatchObject({
      platform: process.platform,
      currentVersion: "1.2.3",
    });
  });
});

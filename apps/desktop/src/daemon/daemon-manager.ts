import { displayAppVersion } from "../app-version.js";
import { app, dialog, ipcMain, powerMonitor } from "electron";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import {
  copyAttachmentFileToManagedStorage,
  deleteManagedAttachmentFile,
  garbageCollectManagedAttachmentFiles,
  readManagedFileBase64,
  writeAttachmentBase64,
  writeAttachmentBytes,
} from "../features/attachments.js";
import {
  checkForAppUpdate,
  downloadAndInstallUpdate,
  type AppReleaseChannel,
  type AppUpdateCheckIntent,
} from "../features/auto-updater.js";
import {
  inspectRetainedDaemonForMigration,
  RustDaemonManager,
  resolveDesktopDaemonHome,
  stopRetainedDaemonForMigration,
  type VerifiedRetainedDaemonProcess,
} from "./rust-daemon.js";
import {
  LegacyDataMigrationCancelledError,
  prepareLegacyDataMigration,
  type LegacyDataMigrationCandidate,
} from "./legacy-data-migration.js";
import { createAccountIpc, stopAccountForExit } from "./account-ipc.js";

import {
  closeLocalTransportSession,
  openLocalTransportSession,
  sendLocalTransportMessage,
} from "./local-transport.js";

import { getDesktopAppLogs } from "../diagnostics/app-logs.js";
import { describeSandbox } from "../diagnostics/sandbox.js";
import { tailFile } from "../diagnostics/tail-file.js";
import { getDesktopUpdaterDiagnostics } from "../diagnostics/updater.js";
import {
  deleteLegacySkillSelection,
  readLegacySkillSelection,
} from "../integrations/legacy-skill-selection.js";
import {
  createDesktopSettingsCommandHandlers,
  type DesktopCommandHandler,
} from "../settings/desktop-settings-commands.js";
import { getDesktopSettingsStore } from "../settings/desktop-settings-electron.js";
import type { DesktopSettings } from "../settings/desktop-settings.js";
import { isRunningUnderARM64Translation } from "../system/arm64-translation.js";

const DAEMON_LOG_FILENAME = "daemon.log";
let manager: RustDaemonManager | null = null;
let dataMigrationPreparation: Promise<void> | null = null;

function getDaemonBinary(): string {
  return (
    process.env.AIT_SERVER_BIN ||
    (app.isPackaged
      ? path.join(
          process.resourcesPath,
          "bin",
          process.platform === "win32" ? "daemon.exe" : "daemon",
        )
      : path.resolve(
          __dirname,
          "../../../../target/debug",
          process.platform === "win32" ? "daemon.exe" : "daemon",
        ))
  );
}

function getRustDaemon(): RustDaemonManager {
  manager ??= new RustDaemonManager({
    binary: getDaemonBinary(),
    home: getDaemonHome(),
    listen: process.env.AIT_SERVER_LISTEN,
    getListen: async () => (await getDesktopSettingsStore().get()).daemon.listen,
  });
  return manager;
}

type DesktopDaemonState = "starting" | "running" | "stopped" | "errored";
const DESKTOP_DAEMON_STOP_REASON_VALUES = [
  "manual_ipc",
  "settings",
  "host_remove",
  "quit",
  "app_update",
  "version_mismatch",
  "restart",
] as const;
export type DesktopDaemonStopReason = (typeof DESKTOP_DAEMON_STOP_REASON_VALUES)[number];

const DESKTOP_DAEMON_STOP_REASONS = new Set<string>(DESKTOP_DAEMON_STOP_REASON_VALUES);
const DEFAULT_DESKTOP_DAEMON_STOP_REASON: DesktopDaemonStopReason = "manual_ipc";

export interface DesktopDaemonStatus {
  serverId: string;
  status: DesktopDaemonState;
  listen: string | null;
  hostname: string | null;
  pid: number | null;
  home: string;
  version: string | null;
  desktopManaged: boolean;
  ownedByDesktop: boolean;
  startedAt: string | null;
  error: string | null;
}

interface DesktopDaemonLogs {
  logPath: string;
  contents: string;
}

function parseReleaseChannel(
  args: Record<string, unknown> | undefined,
): AppReleaseChannel | undefined {
  if (args?.releaseChannel === "beta") {
    return "beta";
  }
  if (args?.releaseChannel === "stable") {
    return "stable";
  }
  return undefined;
}

function parseAppUpdateCheckIntent(
  args: Record<string, unknown> | undefined,
): AppUpdateCheckIntent {
  return args?.intent === "manual" ? "manual" : "automatic";
}

function parseDesktopDaemonStopReason(
  args: Record<string, unknown> | undefined,
): DesktopDaemonStopReason {
  const reason = args?.reason;
  if (typeof reason === "string" && DESKTOP_DAEMON_STOP_REASONS.has(reason)) {
    return reason as DesktopDaemonStopReason;
  }
  return DEFAULT_DESKTOP_DAEMON_STOP_REASON;
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

function getDaemonHome(): string {
  return resolveDesktopDaemonHome(process.env);
}

function logFilePath(): string {
  return path.join(getDaemonHome(), DAEMON_LOG_FILENAME);
}

export function isDesktopManagedDaemonRunningSync(): boolean {
  return manager?.status().ownedByDesktop === true;
}

/** Hand the running managed daemon to the next Desktop session instead of stopping it. */
export async function retainDesktopDaemonForNextSession(): Promise<boolean> {
  if (!manager) return false;
  await stopAccountForExit();
  return manager.retainForNextSession();
}

function resolveDesktopAppVersion(): string {
  if (app.isPackaged) {
    return displayAppVersion();
  }

  try {
    const packageJsonPath = path.join(__dirname, "..", "..", "package.json");
    const pkg = JSON.parse(readFileSync(packageJsonPath, "utf-8")) as {
      version?: unknown;
    };
    if (typeof pkg.version === "string" && pkg.version.trim().length > 0) {
      return pkg.version.trim();
    }
  } catch {
    // Fall back to Electron's default version if the package metadata is unavailable.
  }

  return app.getVersion();
}

// ---------------------------------------------------------------------------
// Daemon lifecycle
// ---------------------------------------------------------------------------

export async function resolveDesktopDaemonStatus(): Promise<DesktopDaemonStatus> {
  return getRustDaemon().status();
}

function assertBuiltInDaemonManagementEnabled(settings: DesktopSettings): void {
  if (process.platform === "win32") {
    throw new Error(
      "The Windows desktop app does not include a built-in daemon. Connect to a remote host.",
    );
  }
  if (!settings.daemon.manageBuiltInDaemon) {
    throw new Error("Built-in daemon management is disabled.");
  }
}

async function startDaemon(): Promise<DesktopDaemonStatus> {
  assertBuiltInDaemonManagementEnabled(await getDesktopSettingsStore().get());
  await prepareDefaultDesktopDataHome();
  return getRustDaemon().start();
}

async function prepareDefaultDesktopDataHome(): Promise<void> {
  if (process.env.AIT_SERVER_DATA_DIR || process.platform === "win32") return;
  if (dataMigrationPreparation) return dataMigrationPreparation;
  const destinationHome = getDaemonHome();
  let confirmedProcesses: VerifiedRetainedDaemonProcess[] = [];
  dataMigrationPreparation = prepareLegacyDataMigration({
    sourceHome: path.join(homedir(), ".ait-server"),
    destinationHome,
    binary: getDaemonBinary(),
    confirm: async (candidate) => {
      const inspected = await Promise.all([
        inspectRetainedDaemonForMigration(candidate.source.home),
        inspectRetainedDaemonForMigration(candidate.destination.home),
      ]);
      confirmedProcesses = inspected.filter(
        (process): process is VerifiedRetainedDaemonProcess => process !== null,
      );
      return confirmLegacyDataMigration(candidate, confirmedProcesses);
    },
    beforeMigrate: async () => {
      for (const process of confirmedProcesses) await stopRetainedDaemonForMigration(process);
    },
  })
    .catch(async (error: unknown) => {
      if (!(error instanceof LegacyDataMigrationCancelledError)) {
        await dialog.showMessageBox({
          type: "error",
          title: "Ait data was not copied / Ait 数据未复制",
          message:
            "Ait left both data directories unchanged. Stop any daemon using the previous directory, then retry. / Ait 未修改两个数据目录。请停止正在使用旧目录的 daemon 后重试。",
          detail: error instanceof Error ? error.message.slice(0, 4000) : String(error),
          buttons: ["OK / 确定"],
          defaultId: 0,
          cancelId: 0,
          noLink: true,
        });
      }
      throw error;
    })
    .finally(() => {
      dataMigrationPreparation = null;
    });
  return dataMigrationPreparation;
}

async function confirmLegacyDataMigration(
  candidate: LegacyDataMigrationCandidate,
  processes: VerifiedRetainedDaemonProcess[] = [],
): Promise<boolean> {
  const processDetails = processes.map((process) => `PID ${process.pid} — ${process.home}`);
  const result = await dialog.showMessageBox({
    type: "question",
    title: "Copy existing Ait data / 复制已有 Ait 数据",
    message:
      "Ait found projects and conversations in the previous data directory. Copy them into the new desktop data directory? / Ait 在旧数据目录中发现了项目和会话。是否复制到新的桌面数据目录？",
    detail: [
      `${candidate.source.agents} Agents, ${candidate.source.projects} projects, ${candidate.source.workspaces} Workspaces`,
      `${candidate.source.home} → ${candidate.destination.home}`,
      ...(processes.length > 0
        ? [
            "Ait verified the following old daemon process. Confirming will stop it before copying; shutdown is attempted before signals. / Ait 已核验以下旧 daemon 进程。确认后会先停止它再复制，并优先尝试正常关闭。",
            ...processDetails,
          ]
        : [
            "Both the source and the previous destination are preserved. Any unverified daemon must be stopped before copying. / 源目录和目标目录原内容都会保留。复制前必须停止无法核验的 daemon。",
          ]),
    ].join("\n"),
    buttons: [
      "Not now / 暂不处理",
      processes.length > 0
        ? "Stop old process and copy / 停止旧进程并复制"
        : "Copy data / 复制数据",
    ],
    defaultId: 1,
    cancelId: 0,
    noLink: true,
  });
  return result.response === 1;
}

export async function stopDesktopDaemon(
  _reason: DesktopDaemonStopReason = DEFAULT_DESKTOP_DAEMON_STOP_REASON,
  confirmedInstance?: { pid: number; startedAt: string },
): Promise<DesktopDaemonStatus> {
  if (_reason === "quit" || _reason === "app_update") await stopAccountForExit();
  return getRustDaemon().stop(confirmedInstance);
}

async function restartDaemon(): Promise<DesktopDaemonStatus> {
  assertBuiltInDaemonManagementEnabled(await getDesktopSettingsStore().get());
  await prepareDefaultDesktopDataHome();
  return getRustDaemon().restart();
}

function getDaemonLogs(): DesktopDaemonLogs {
  const logPath = logFilePath();
  return {
    logPath,
    contents: tailFile(logPath, 100),
  };
}

async function getCliDaemonStatus(): Promise<string> {
  return JSON.stringify(await resolveDesktopDaemonStatus(), null, 2);
}

async function getLocalDaemonVersion(): Promise<{ version: string | null; error: string | null }> {
  const status = await resolveDesktopDaemonStatus();
  if (status.status !== "running") {
    return { version: null, error: "Daemon is not running." };
  }
  return {
    version: status.version,
    error: status.version ? null : "Running daemon did not report a version.",
  };
}

async function resolveRequestedReleaseChannel(
  args: Record<string, unknown> | undefined,
): Promise<AppReleaseChannel> {
  return parseReleaseChannel(args) ?? (await getDesktopSettingsStore().get()).releaseChannel;
}

// ---------------------------------------------------------------------------
// IPC registration
// ---------------------------------------------------------------------------

export function createDaemonCommandHandlers(): Record<string, DesktopCommandHandler> {
  return {
    ...createDesktopSettingsCommandHandlers({ settingsStore: getDesktopSettingsStore() }),
    desktop_get_runtime_info: () => ({
      appVersion: resolveDesktopAppVersion(),
      runningUnderARM64Translation: isRunningUnderARM64Translation(),
    }),
    desktop_daemon_status: () => resolveDesktopDaemonStatus(),
    start_desktop_daemon: () => startDaemon(),
    stop_desktop_daemon: (args) =>
      stopDesktopDaemon(
        parseDesktopDaemonStopReason(args),
        typeof args?.pid === "number" && typeof args.startedAt === "string"
          ? { pid: args.pid, startedAt: args.startedAt }
          : undefined,
      ),
    restart_desktop_daemon: () => restartDaemon(),
    desktop_daemon_logs: () => getDaemonLogs(),
    desktop_sandbox_diagnostics: () =>
      describeSandbox({
        disabled: app.commandLine.hasSwitch("no-sandbox"),
        launcherReason: process.env.AIT_DESKTOP_SANDBOX_REASON,
      }),
    desktop_app_logs: () => getDesktopAppLogs(),
    desktop_update_diagnostics: () => getDesktopUpdaterDiagnostics(),
    desktop_get_system_idle_time: () => powerMonitor.getSystemIdleTime() * 1000,
    cli_daemon_status: () => getCliDaemonStatus(),
    write_attachment_base64: (args) => writeAttachmentBase64(args ?? {}),
    write_attachment_bytes: (args) => writeAttachmentBytes(args ?? {}),
    copy_attachment_file: (args) => copyAttachmentFileToManagedStorage(args ?? {}),
    read_file_base64: (args) => readManagedFileBase64(args ?? {}),
    delete_attachment_file: (args) => deleteManagedAttachmentFile(args ?? {}),
    garbage_collect_attachment_files: (args) => garbageCollectManagedAttachmentFiles(args ?? {}),
    open_local_daemon_transport: async (args) => {
      const target = args?.target as { transportType?: string; url?: string } | undefined;
      const token =
        target?.transportType === "rustTcp" && typeof target.url === "string"
          ? manager?.authorization(target.url)
          : undefined;
      return openLocalTransportSession(
        token
          ? {
              ...args,
              bearerToken: token,
              target: {
                transportType: "rustTcp",
                url: `ws://${manager!.status().connectAddress ?? manager!.status().listen}/v1/ws`,
              },
            }
          : args,
      );
    },
    send_local_daemon_transport_message: async (args) => {
      await sendLocalTransportMessage(
        args as { sessionId: string; text?: string; binaryBase64?: string },
      );
    },
    close_local_daemon_transport: (args) => {
      const sessionId =
        typeof args === "object" && args !== null && "sessionId" in args
          ? (args as { sessionId: string }).sessionId
          : "";
      if (sessionId) closeLocalTransportSession(sessionId);
    },
    check_app_update: async (args) => {
      const currentVersion = resolveDesktopAppVersion();
      return checkForAppUpdate({
        currentVersion,
        releaseChannel: await resolveRequestedReleaseChannel(args),
        intent: parseAppUpdateCheckIntent(args),
      });
    },
    install_app_update: async (args) => {
      const currentVersion = resolveDesktopAppVersion();
      return downloadAndInstallUpdate(
        { currentVersion, releaseChannel: await resolveRequestedReleaseChannel(args) },
        async () => {
          await stopDesktopDaemon("app_update");
        },
      );
    },
    get_local_daemon_version: () => getLocalDaemonVersion(),
    read_legacy_skill_selection: () => readLegacySkillSelection(),
    delete_legacy_skill_selection: () => deleteLegacySkillSelection(),
  };
}

export function registerDaemonManager(
  isAccountAppWindow: (contents: Electron.WebContents) => boolean,
): void {
  const handlers = createDaemonCommandHandlers();
  const account = createAccountIpc(getRustDaemon, isAccountAppWindow);

  ipcMain.handle("ait:invoke", async (_event, command: string, args?: Record<string, unknown>) => {
    if (command.startsWith("account_")) return account(_event, command, args);
    const handler = Object.hasOwn(handlers, command) ? handlers[command] : undefined;
    if (!handler) {
      throw new Error(`Unknown desktop command: ${command}`);
    }
    return await handler(args);
  });
}

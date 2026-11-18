import { app, type BrowserWindow } from "electron";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";

// Electron can emit a single unresponsive event for a continuous hang.
const HANG_GRACE_MS = 15_000;
const RECOVERY_COOLDOWN_MS = 60_000;
const MAX_CONSECUTIVE_RECOVERIES = 3;
const HANG_SNAPSHOT_DIR_NAME = "renderer-hang-snapshots";

// Collect evidence the next diagnosis needs: what the renderer was doing when
// it hung. Everything gathered here must survive a forced crash.
async function collectHangSnapshot(win: BrowserWindow, attempt: number): Promise<void> {
  const stamp = new Date().toISOString().replace(/[:.]/g, "-");
  const directory = path.join(app.getPath("userData"), HANG_SNAPSHOT_DIR_NAME);
  const lines: string[] = [
    `time: ${new Date().toISOString()}`,
    `webContentsId: ${win.webContents.id}`,
    `url: ${win.webContents.getURL()}`,
    `attempt: ${attempt}`,
    `rendererPid: ${win.webContents.getOSProcessId()}`,
    `appMetrics: ${JSON.stringify(app.getAppMetrics())}`,
  ];
  await mkdir(directory, { recursive: true });
  await writeFile(path.join(directory, `${stamp}.json`), `${lines.join("\n")}\n`);
}

export function setupRendererHangWatchdog(win: BrowserWindow): void {
  const contents = win.webContents;
  let hangTimer: NodeJS.Timeout | undefined;
  let reloadOnExit = false;
  let nextRecoveryAt = 0;
  let loadedAt: number | undefined;
  let consecutiveRecoveries = 0;

  function cancelHang() {
    clearTimeout(hangTimer);
    hangTimer = undefined;
  }

  function invalidateDocument() {
    cancelHang();
    reloadOnExit = false;
    loadedAt = undefined;
  }

  async function recover(timer: NodeJS.Timeout) {
    if (consecutiveRecoveries >= MAX_CONSECUTIVE_RECOVERIES) return;

    const attempt = consecutiveRecoveries + 1;
    try {
      await collectHangSnapshot(win, attempt);
    } catch (error) {
      console.warn("[renderer-hang-watchdog] Snapshot collection failed", error);
    }
    // The window may have recovered, navigated, closed, or started a new hang during I/O.
    if (hangTimer !== timer || win.isDestroyed()) return;

    cancelHang();
    consecutiveRecoveries = attempt;
    nextRecoveryAt = Date.now() + RECOVERY_COOLDOWN_MS;
    reloadOnExit = true;
    console.warn(`[renderer-hang-watchdog] Restarting unresponsive renderer (attempt ${attempt})`);
    contents.forcefullyCrashRenderer();
  }

  win.on("unresponsive", () => {
    if (hangTimer || reloadOnExit || win.isDestroyed()) return;
    const now = Date.now();
    // A replacement document can stay healthy for days without ever emitting "responsive".
    if (loadedAt !== undefined && now - loadedAt >= RECOVERY_COOLDOWN_MS) consecutiveRecoveries = 0;
    loadedAt = undefined;
    const delay = Math.max(HANG_GRACE_MS, nextRecoveryAt - now);
    const timer = setTimeout(() => void recover(timer), delay);
    hangTimer = timer;
  });
  win.on("responsive", () => {
    if (hangTimer) {
      cancelHang();
      consecutiveRecoveries = 0;
    }
  });
  contents.on("render-process-gone", () => {
    const shouldReload = reloadOnExit;
    invalidateDocument();
    // On Linux the forced exit is asynchronous. Reloading earlier can target the dying process.
    if (shouldReload && !win.isDestroyed()) contents.reload();
  });
  contents.on("did-navigate", invalidateDocument);
  contents.on("did-finish-load", () => {
    loadedAt = Date.now();
  });
  win.once("closed", invalidateDocument);
}

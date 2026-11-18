import { app, type BrowserWindow } from "electron";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";

// The renderer main thread can livelock (JS/GC allocation storm) while the
// process and the compositor stay alive: the window paints nothing new, input
// goes unprocessed, and every other watchdog sees a healthy process. Electron's
// `unresponsive` event is the only main-process signal for this state.

// Electron fires `unresponsive` repeatedly while the renderer stays hung; this
// many consecutive firings (each ~15 s apart in practice) means a hard hang.
const UNRESPONSIVE_EVENTS_TO_RECOVER = 2;
// Minimum gap between forced renderer recoveries.
const RECOVERY_COOLDOWN_MS = 60_000;
// Stop force-crashing the renderer after this many tries without a `responsive`
// event; the user gets to keep (or close) whatever window state remains.
const MAX_CONSECUTIVE_RECOVERIES = 3;
// Where the diagnostic snapshots taken before recovery land.
const HANG_SNAPSHOT_DIR_NAME = "renderer-hang-snapshots";
// Grace period for the reloaded renderer to answer before probing resumes.
const RECOVERY_GRACE_MS = 5_000;

export interface HangWatchdogState {
  unresponsiveEvents: number;
  recovering: boolean;
  msSinceLastRecovery: number;
  consecutiveRecoveries: number;
}

export function shouldRecoverFromHang(state: HangWatchdogState): boolean {
  return (
    state.unresponsiveEvents >= UNRESPONSIVE_EVENTS_TO_RECOVER &&
    !state.recovering &&
    state.msSinceLastRecovery >= RECOVERY_COOLDOWN_MS &&
    state.consecutiveRecoveries < MAX_CONSECUTIVE_RECOVERIES
  );
}

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
  let unresponsiveEvents = 0;
  let recovering = false;
  let lastRecoveryAt = 0;
  let consecutiveRecoveries = 0;

  const recoverRenderer = async () => {
    recovering = true;
    lastRecoveryAt = Date.now();
    consecutiveRecoveries += 1;
    unresponsiveEvents = 0;
    const attempt = consecutiveRecoveries;
    console.warn(
      `[renderer-hang-watchdog] Renderer unresponsive ${UNRESPONSIVE_EVENTS_TO_RECOVER}+ times; ` +
        `collecting snapshot and force-crashing renderer (attempt ${attempt})`,
    );
    try {
      await collectHangSnapshot(win, attempt);
    } catch (error) {
      console.warn("[renderer-hang-watchdog] Snapshot collection failed", error);
    }
    if (win.isDestroyed()) {
      recovering = false;
      return;
    }
    // A forced crash reloads the renderer through the same render-process-gone
    // handling as any crash, preserving the loaded URL.
    win.webContents.forcefullyCrashRenderer();
    // Re-arm after a grace period; `responsive` resets the counters when the
    // reloaded renderer answers.
    setTimeout(() => {
      recovering = false;
    }, RECOVERY_GRACE_MS);
  };

  win.on("unresponsive", () => {
    if (win.isDestroyed()) return;
    unresponsiveEvents += 1;
    if (
      shouldRecoverFromHang({
        unresponsiveEvents,
        recovering,
        msSinceLastRecovery: Date.now() - lastRecoveryAt,
        consecutiveRecoveries,
      })
    ) {
      void recoverRenderer();
    }
  });
  win.on("responsive", () => {
    unresponsiveEvents = 0;
    consecutiveRecoveries = 0;
  });
}

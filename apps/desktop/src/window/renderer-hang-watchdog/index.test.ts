import type { BrowserWindow } from "electron";
import { EventEmitter } from "node:events";
import { writeFile } from "node:fs/promises";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setupRendererHangWatchdog } from "./index.js";

vi.mock("electron", () => ({
  app: { getPath: () => "/profile", getAppMetrics: () => [] },
}));
vi.mock("node:fs/promises", () => ({ mkdir: vi.fn(), writeFile: vi.fn() }));

function watchWindow() {
  const contents = Object.assign(new EventEmitter(), {
    id: 1,
    getURL: () => "app://app/",
    getOSProcessId: () => 42,
    forcefullyCrashRenderer: vi.fn(),
    reload: vi.fn(),
  });
  const win = Object.assign(new EventEmitter(), {
    isDestroyed: () => false,
    webContents: contents,
  });
  setupRendererHangWatchdog(win as unknown as BrowserWindow);
  return { win, contents };
}

describe("renderer-hang-watchdog", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.spyOn(console, "warn").mockImplementation(() => {});
  });
  afterEach(() => {
    vi.clearAllMocks();
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it("waits from the first alert, snapshots once and reloads only after the forced exit", async () => {
    const { win, contents } = watchWindow();
    win.emit("unresponsive");
    await vi.advanceTimersByTimeAsync(14_999);
    expect(writeFile).not.toHaveBeenCalled();
    win.emit("unresponsive");
    await vi.advanceTimersByTimeAsync(1);
    expect(writeFile).toHaveBeenCalledOnce();
    expect(contents.forcefullyCrashRenderer).toHaveBeenCalledOnce();
    expect(contents.reload).not.toHaveBeenCalled();
    win.emit("responsive");
    contents.emit("render-process-gone");
    expect(contents.reload).toHaveBeenCalledOnce();
    contents.emit("render-process-gone");
    expect(contents.reload).toHaveBeenCalledOnce();
  });

  it("recovers from a single alert even if saving diagnostics fails", async () => {
    vi.mocked(writeFile).mockRejectedValueOnce(new Error("disk full"));
    const { win, contents } = watchWindow();
    win.emit("unresponsive");
    await vi.advanceTimersByTimeAsync(15_000);
    expect(contents.forcefullyCrashRenderer).toHaveBeenCalledOnce();
  });

  it.each(["responsive", "closed", "did-navigate", "render-process-gone"])(
    "cancels a pending recovery on %s",
    async (event) => {
      const { win, contents } = watchWindow();
      win.emit("unresponsive");
      (event === "responsive" || event === "closed" ? win : contents).emit(event);
      await vi.advanceTimersByTimeAsync(15_000);
      expect(writeFile).not.toHaveBeenCalled();
      expect(contents.forcefullyCrashRenderer).not.toHaveBeenCalled();
      expect(contents.reload).not.toHaveBeenCalled();
    },
  );

  it("discards a stale snapshot even when another hang has started", async () => {
    let finishWrite!: () => void;
    vi.mocked(writeFile).mockReturnValueOnce(
      new Promise<void>((resolve) => {
        finishWrite = resolve;
      }),
    );
    const { win, contents } = watchWindow();
    win.emit("unresponsive");
    await vi.advanceTimersByTimeAsync(15_000);
    expect(writeFile).toHaveBeenCalledOnce();
    win.emit("responsive");
    win.emit("unresponsive");
    finishWrite();
    await vi.advanceTimersByTimeAsync(14_999);
    expect(contents.forcefullyCrashRenderer).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(contents.forcefullyCrashRenderer).toHaveBeenCalledOnce();
  });

  it("delays new hangs through the cooldown and caps restarts until a hang clears", async () => {
    const { win, contents } = watchWindow();
    for (let attempt = 1; attempt <= 3; attempt++) {
      win.emit("unresponsive");
      await vi.advanceTimersByTimeAsync((attempt === 1 ? 15_000 : 60_000) - 1);
      expect(contents.forcefullyCrashRenderer).toHaveBeenCalledTimes(attempt - 1);
      await vi.advanceTimersByTimeAsync(1);
      expect(contents.forcefullyCrashRenderer).toHaveBeenCalledTimes(attempt);
      contents.emit("render-process-gone");
      contents.emit("did-navigate");
      contents.emit("did-finish-load");
    }
    win.emit("unresponsive");
    await vi.advanceTimersByTimeAsync(60_000);
    expect(writeFile).toHaveBeenCalledTimes(3);
    win.emit("responsive");
    win.emit("unresponsive");
    await vi.advanceTimersByTimeAsync(15_000);
    expect(contents.forcefullyCrashRenderer).toHaveBeenCalledTimes(4);
  });

  it("recovers isolated hangs after the replacement page has been healthy for a day", async () => {
    const { win, contents } = watchWindow();
    for (let attempt = 1; attempt <= 4; attempt++) {
      win.emit("unresponsive");
      await vi.advanceTimersByTimeAsync(15_000);
      expect(contents.forcefullyCrashRenderer).toHaveBeenCalledTimes(attempt);
      contents.emit("render-process-gone");
      contents.emit("did-navigate");
      contents.emit("did-finish-load");
      await vi.advanceTimersByTimeAsync(86_400_000);
    }
  });
});

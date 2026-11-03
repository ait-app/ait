import { EventEmitter } from "node:events";
import type { DownloadItem, WebContents } from "electron";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DirectDownloadManager } from "./direct-download.js";

const url = "http://127.0.0.1:7316/api/files/download?token=one-use";
function fixture() {
  const session = new EventEmitter();
  const owner = Object.assign(new EventEmitter(), {
    session,
    isDestroyed: () => false,
    downloadURL: vi.fn(),
    send: vi.fn(),
  });
  const item = Object.assign(new EventEmitter(), {
    getURLChain: () => [url],
    getReceivedBytes: () => 8,
    getTotalBytes: () => 8,
    setSaveDialogOptions: vi.fn(),
    cancel: vi.fn(),
  });
  const manager = new DirectDownloadManager();
  return {
    owner,
    item,
    start: () =>
      manager.download(owner as unknown as WebContents, {
        url,
        fileName: "report.zip",
        downloadId: "download",
      }),
    nativeStart: () => session.emit("will-download", {}, item as unknown as DownloadItem, owner),
  };
}
afterEach(() => vi.useRealTimers());

describe("direct desktop downloads", () => {
  it("keeps the app page in place and waits for the native disk completion", async () => {
    vi.useFakeTimers();
    const { owner, item, start, nativeStart } = fixture();
    let completed = false;
    const pending = start().then(() => {
      completed = true;
    });
    expect(owner.downloadURL).toHaveBeenCalledExactlyOnceWith(url);
    nativeStart();
    // The user may take longer than the one-use token lifetime to choose a destination.
    await vi.advanceTimersByTimeAsync(300_000);
    expect(completed).toBe(false);
    item.emit("updated");
    expect(owner.send).toHaveBeenCalledWith("paseo:event:direct-download-progress", {
      id: "download",
      bytesWritten: 8,
      totalBytes: 8,
    });
    item.emit("done", {}, "completed");
    await pending;
    expect(completed).toBe(true);
    expect(item.setSaveDialogOptions).toHaveBeenCalledWith({ defaultPath: "report.zip" });
    expect(owner.session.listenerCount("will-download")).toBe(0);
    expect(owner.listenerCount("destroyed")).toBe(0);
    expect(item.listenerCount("updated")).toBe(0);
  });

  it.each(["cancelled", "interrupted"])(
    "rejects native %s instead of reporting success",
    async (state) => {
      const { item, start, nativeStart } = fixture();
      const result = expect(start()).rejects.toThrow("Download interrupted.");
      nativeStart();
      item.emit("done", {}, state);
      await result;
    },
  );

  it("isolates other windows and cancels transfers when their owner navigates", async () => {
    const { owner, item, start, nativeStart } = fixture();
    const result = expect(start()).rejects.toThrow("Download cancelled.");
    owner.session.emit("will-download", {}, item, {});
    expect(item.setSaveDialogOptions).not.toHaveBeenCalled();
    nativeStart();
    owner.emit("did-start-navigation", {}, "app://other", false, true);
    await result;
    expect(item.cancel).toHaveBeenCalledOnce();
    expect(owner.listenerCount("did-start-navigation")).toBe(0);
  });

  it.each([
    "file:///etc/passwd",
    "https://host.example/other?token=x",
    "https://host.example/api/files/download",
    "https://host.example/api/files/download?token=x&path=/etc/passwd",
  ])("rejects unsupported URL %s", async (value) => {
    const { owner } = fixture();
    await expect(
      new DirectDownloadManager().download(owner as unknown as WebContents, {
        url: value,
        downloadId: "download",
      }),
    ).rejects.toThrow("Invalid download URL.");
    expect(owner.downloadURL).not.toHaveBeenCalled();
  });

  it("releases pending startup when Electron never accepts the transfer", async () => {
    vi.useFakeTimers();
    const { start, owner } = fixture();
    const result = expect(start()).rejects.toThrow("Download connection timed out.");
    await vi.advanceTimersByTimeAsync(30_000);
    await result;
    expect(owner.session.listenerCount("will-download")).toBe(0);
    expect(owner.listenerCount("destroyed")).toBe(0);
  });

  it("bounds concurrent preparations and releases slots after their window closes", async () => {
    const { owner } = fixture();
    const manager = new DirectDownloadManager();
    const start = () =>
      manager.download(owner as unknown as WebContents, { url, downloadId: "download" });
    const pending = Array.from({ length: 4 }, () =>
      expect(start()).rejects.toThrow("Download cancelled."),
    );
    await expect(start()).rejects.toThrow("Too many concurrent downloads.");
    owner.emit("destroyed");
    await Promise.all(pending);
    const next = expect(start()).rejects.toThrow("Download cancelled.");
    owner.emit("destroyed");
    await next;
  });

  it("cancels a stalled native transfer and removes all pending listeners", async () => {
    vi.useFakeTimers();
    const { item, start, nativeStart } = fixture();
    const result = expect(start()).rejects.toThrow("Download stalled.");
    nativeStart();
    item.emit("updated");
    await vi.advanceTimersByTimeAsync(30_000);
    await result;
    expect(item.cancel).toHaveBeenCalledOnce();
    expect(item.listenerCount("done")).toBe(0);
  });
});

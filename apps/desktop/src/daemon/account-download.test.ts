import { EventEmitter } from "node:events";
import { open, rename } from "node:fs/promises";
import { dialog, type WebContents } from "electron";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AccountDownloadManager } from "./account-download.js";
import type { AccountSessionManager } from "./account-session.js";

vi.mock("electron", () => ({
  BrowserWindow: { fromWebContents: () => ({}) },
  dialog: { showSaveDialog: vi.fn() },
}));
vi.mock("node:fs/promises", () => ({
  open: vi.fn(async () => ({
    sync: vi.fn(async () => undefined),
    close: vi.fn(async () => undefined),
  })),
  rename: vi.fn(async () => undefined),
  unlink: vi.fn(async () => undefined),
}));
vi.mock("ws", async () => {
  const { EventEmitter } = await import("node:events");
  return {
    WebSocket: class extends EventEmitter {
      constructor() {
        super();
        queueMicrotask(() => {
          for (const frame of [
            { type: "relay.ready", relay_session_id: "visit" },
            { type: "download.headers", status: 200, content_length: 0 },
            { type: "download.end", bytes: 0 },
          ])
            this.emit("message", Buffer.from(JSON.stringify(frame)), false);
        });
      }
      pause() {}
      resume() {}
      terminate() {}
      send(_message: string, callback: () => void) {
        callback();
      }
    },
  };
});

function webContents() {
  return Object.assign(new EventEmitter(), {
    isDestroyed: () => false,
    send: vi.fn(),
  }) as unknown as WebContents;
}

const input = { hostId: "host", fileName: "report.txt", downloadId: "download" };
let manager: AccountDownloadManager;
let owner: WebContents;
const openDownload = vi.fn(async () => ({
  relay_session_id: "visit",
  client_ticket: "ticket",
  url: "wss://center.example/download",
}));

beforeEach(() => {
  vi.clearAllMocks();
  vi.useFakeTimers();
  vi.mocked(dialog.showSaveDialog).mockResolvedValue({
    canceled: false,
    filePath: "/chosen/report.txt",
  });
  owner = webContents();
  manager = new AccountDownloadManager({
    snapshot: () => ({ selected: { host_id: "host" } }),
    openDownload,
    closeVisit: vi.fn(async () => undefined),
  } as unknown as AccountSessionManager);
});
afterEach(() => {
  manager.closeAll();
  vi.useRealTimers();
});

describe("account download preparations", () => {
  it("keeps the selected path in main and starts a one-use transfer after a long save dialog", async () => {
    let confirm!: (result: Electron.SaveDialogReturnValue) => void;
    vi.mocked(dialog.showSaveDialog).mockReturnValueOnce(
      new Promise((resolve) => {
        confirm = resolve;
      }),
    );
    const preparing = manager.prepare(owner, input);
    await vi.advanceTimersByTimeAsync(360_000);
    expect(openDownload).not.toHaveBeenCalled();
    expect(open).not.toHaveBeenCalled();

    confirm({ canceled: false, filePath: "/chosen/report.txt" });
    const preparationId = await preparing;
    expect(preparationId).not.toContain("/chosen");
    await manager.download(owner, { preparationId, token: "fresh-token" });
    expect(openDownload).toHaveBeenCalledExactlyOnceWith("host", "fresh-token");
    expect(dialog.showSaveDialog).toHaveBeenCalledOnce();
    expect(rename).toHaveBeenCalledWith(
      expect.stringMatching(/^\/chosen\/report\.txt\.ait-.*\.part$/),
      "/chosen/report.txt",
    );
    await expect(manager.download(owner, { preparationId, token: "reused" })).rejects.toThrow(
      "Invalid download preparation.",
    );
    expect(owner.listenerCount("destroyed")).toBe(0);
    expect(owner.listenerCount("did-start-navigation")).toBe(0);
  });

  it("rejects preparation use by another window without consuming the owner's selection", async () => {
    const preparationId = await manager.prepare(owner, input);
    const other = webContents();
    manager.cancel(other, preparationId);
    await expect(manager.download(other, { preparationId, token: "token" })).rejects.toThrow(
      "Invalid download preparation.",
    );
    expect(openDownload).not.toHaveBeenCalled();
    await manager.download(owner, { preparationId, token: "token" });
    expect(openDownload).toHaveBeenCalledOnce();
  });

  it.each(["cancel", "closeAll", "navigation", "destroyed", "expiry"])(
    "invalidates the preparation on %s before any relay session opens",
    async (reason) => {
      const preparationId = await manager.prepare(owner, input);
      if (reason === "cancel") manager.cancel(owner, preparationId);
      if (reason === "closeAll") manager.closeAll();
      if (reason === "navigation") owner.emit("did-start-navigation", {}, "app://new", false, true);
      if (reason === "destroyed") owner.emit("destroyed");
      if (reason === "expiry") await vi.advanceTimersByTimeAsync(300_000);
      await expect(manager.download(owner, { preparationId, token: "token" })).rejects.toThrow(
        "Invalid download preparation.",
      );
      expect(openDownload).not.toHaveBeenCalled();
      expect(owner.listenerCount("destroyed")).toBe(0);
      expect(owner.listenerCount("did-start-navigation")).toBe(0);
    },
  );

  it("invalidates an open dialog when the account closes its downloads", async () => {
    let confirm!: (result: Electron.SaveDialogReturnValue) => void;
    vi.mocked(dialog.showSaveDialog).mockReturnValueOnce(
      new Promise((resolve) => {
        confirm = resolve;
      }),
    );
    const preparing = manager.prepare(owner, input);
    manager.closeAll();
    confirm({ canceled: false, filePath: "/chosen/report.txt" });
    await expect(preparing).rejects.toThrow("Download cancelled.");
    expect(openDownload).not.toHaveBeenCalled();
  });

  it("counts preparations toward the concurrency limit and releases cancelled slots", async () => {
    const ids = await Promise.all(Array.from({ length: 4 }, () => manager.prepare(owner, input)));
    await expect(manager.prepare(owner, input)).rejects.toThrow("Too many concurrent downloads.");
    manager.cancel(owner, ids[0]!);
    await expect(manager.prepare(owner, input)).resolves.toEqual(expect.any(String));
  });
});

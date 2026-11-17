import { EventEmitter } from "node:events";
import { open, rename, unlink } from "node:fs/promises";
import { dialog, type WebContents } from "electron";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { HubSessionClosedError } from "@ait/client/internal/hub";
import { AccountDownloadManager } from "./account-download.js";
import type { AccountSessionManager } from "./account-session.js";

const relay = vi.hoisted(() => ({ length: 0 as number | null }));
vi.mock("electron", () => ({
  BrowserWindow: { fromWebContents: () => ({}) },
  dialog: { showSaveDialog: vi.fn() },
}));
vi.mock("node:fs/promises", () => ({
  open: vi.fn(async () => ({
    sync: vi.fn(async () => undefined),
    close: vi.fn(async () => undefined),
    truncate: vi.fn(async () => undefined),
    write: vi.fn(async (chunk: Buffer, offset: number, length: number) => ({
      bytesWritten: Math.min(length, chunk.length - offset),
    })),
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
            { type: "download.headers", status: 200, content_length: relay.length },
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

const input = {
  hostId: "host",
  center: "https://center.example/api",
  fileName: "report.txt",
  downloadId: "download",
};
let manager: AccountDownloadManager;
let owner: WebContents;
const openDownload = vi.fn(async () => ({
  relay_session_id: "visit",
  client_ticket: "ticket",
  url: "wss://center.example/download",
}));

beforeEach(() => {
  vi.clearAllMocks();
  relay.length = 0;
  vi.useFakeTimers();
  vi.mocked(dialog.showSaveDialog).mockResolvedValue({
    canceled: false,
    filePath: "/chosen/report.txt",
  });
  owner = webContents();
  manager = new AccountDownloadManager({
    snapshot: () => ({ status: "online", center: input.center, selected: null }),
    openDownload,
    closeVisit: vi.fn(async () => undefined),
  } as unknown as AccountSessionManager);
});
afterEach(() => {
  manager.closeAll();
  vi.useRealTimers();
});

describe("account download preparations", () => {
  it("rejects a saved host from a different service before opening the save dialog", async () => {
    await expect(
      manager.prepare(owner, { ...input, center: "https://other.test/api" }),
    ).rejects.toThrow("Sign in to the online service used by this host.");
    expect(dialog.showSaveDialog).not.toHaveBeenCalled();
    expect(openDownload).not.toHaveBeenCalled();
  });
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
    expect(openDownload).toHaveBeenCalledExactlyOnceWith("host", "fresh-token", input.center);
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

  it.each([0, null])(
    "rejects a short receipt-bound relay download with length %s",
    async (length) => {
      relay.length = length;
      const preparationId = await manager.prepare(owner, { ...input, expectedSize: 4 });
      await expect(manager.download(owner, { preparationId, token: "once" })).rejects.toThrow(
        /mismatch|incomplete/,
      );
      expect(rename).not.toHaveBeenCalled();
      expect(unlink).toHaveBeenCalledWith(expect.stringMatching(/\.part$/));
      expect(openDownload).toHaveBeenCalledOnce();
    },
  );

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

describe("account Hub downloads", () => {
  function hubSession() {
    const handlers = {
      message: (_data: unknown, _binary: boolean) => {},
      error: (_error: unknown) => {},
    };
    return {
      session: {
        send: vi.fn(),
        consumed: vi.fn(),
        end: vi.fn(),
        onMessage: vi.fn((handler: typeof handlers.message) => {
          handlers.message = handler;
          return () => {};
        }),
        onError: vi.fn((handler: typeof handlers.error) => {
          handlers.error = handler;
          return () => {};
        }),
      },
      text: (value: object) => handlers.message(JSON.stringify(value), false),
      binary: (value: Uint8Array) => handlers.message(value, true),
      close: (code: string) => handlers.error(new HubSessionClosedError(code)),
    };
  }

  function hubManager(openHubDownload: () => Promise<unknown>) {
    return new AccountDownloadManager({
      snapshot: () => ({ status: "online", center: input.center, selected: null }),
      openDownload,
      openHubDownload: vi.fn(openHubDownload),
      closeVisit: vi.fn(async () => undefined),
    } as unknown as AccountSessionManager);
  }

  it("streams over a Hub session, grants credit after writes and finishes on done", async () => {
    const hub = hubSession();
    const local = hubManager(async () => hub.session);
    const preparationId = await local.prepare(owner, input);
    const downloading = local.download(owner, { preparationId, token: "token" });
    await vi.advanceTimersByTimeAsync(0);
    hub.text({ type: "download.headers", status: 200, content_length: 3 });
    hub.binary(new Uint8Array([1, 2, 3]));
    hub.text({ type: "download.end", bytes: 3 });
    await vi.advanceTimersByTimeAsync(0);
    expect(hub.session.consumed).toHaveBeenCalledWith(3);
    expect(hub.session.send).toHaveBeenCalledWith('{"type":"download.complete"}');
    hub.close("done");
    await downloading;
    expect(openDownload).not.toHaveBeenCalled();
    expect(rename).toHaveBeenCalledWith(expect.any(String), "/chosen/report.txt");
    expect(hub.session.end).toHaveBeenCalledWith("closed");
  });

  it("rejects a short Hub artifact instead of acknowledging or renaming it", async () => {
    const hub = hubSession();
    const local = hubManager(async () => hub.session);
    const preparationId = await local.prepare(owner, { ...input, expectedSize: 4 });
    const downloading = local.download(owner, { preparationId, token: "once" });
    const failed = expect(downloading).rejects.toThrow(/incomplete/);
    await vi.advanceTimersByTimeAsync(0);
    hub.text({ type: "download.headers", status: 200, content_length: null });
    hub.binary(new Uint8Array([1, 2, 3]));
    hub.text({ type: "download.end", bytes: 3 });
    await failed;
    expect(hub.session.send).not.toHaveBeenCalled();
    expect(rename).not.toHaveBeenCalled();
    expect(unlink).toHaveBeenCalledWith(expect.stringMatching(/\.part$/));
    expect(openDownload).not.toHaveBeenCalled();
    local.closeAll();
  });

  it("listens before the Host can answer, however slowly the file opens", async () => {
    const opened = vi.mocked(open).getMockImplementation()!;
    vi.mocked(open).mockImplementationOnce(async (...args) => {
      await new Promise((resolve) => setTimeout(resolve, 50));
      return opened(...args);
    });
    const hub = hubSession();
    const local = hubManager(async () => {
      // A fast Host answers in the next task; the session buffers nothing.
      setTimeout(() => {
        hub.text({ type: "download.headers", status: 200, content_length: 3 });
        hub.binary(new Uint8Array([1, 2, 3]));
        hub.text({ type: "download.end", bytes: 3 });
      }, 0);
      return hub.session;
    });
    const preparationId = await local.prepare(owner, input);
    const downloading = local.download(owner, { preparationId, token: "token" });
    await vi.advanceTimersByTimeAsync(100);
    expect(hub.session.send).toHaveBeenCalledWith('{"type":"download.complete"}');
    hub.close("done");
    await downloading;
    expect(rename).toHaveBeenCalledWith(expect.any(String), "/chosen/report.txt");
  });

  it("falls back to the relay route when the Host answers unsupported", async () => {
    const hub = hubSession();
    const local = hubManager(async () => hub.session);
    const preparationId = await local.prepare(owner, input);
    const downloading = local.download(owner, { preparationId, token: "token" });
    await vi.advanceTimersByTimeAsync(0);
    hub.close("unsupported");
    await downloading;
    expect(openDownload).toHaveBeenCalledExactlyOnceWith("host", "token", input.center);
  });

  it("uses the relay route when the Host lacks Hub downloads", async () => {
    const local = hubManager(async () => null);
    const preparationId = await local.prepare(owner, input);
    await local.download(owner, { preparationId, token: "token" });
    expect(openDownload).toHaveBeenCalledOnce();
  });
});

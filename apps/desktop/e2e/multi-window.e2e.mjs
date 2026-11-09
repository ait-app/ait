import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";
import { _electron as electron } from "playwright";

const require = createRequire(import.meta.url);
const desktop = fileURLToPath(new URL("..", import.meta.url));
const root = path.resolve(desktop, "../..");
const temporary = mkdtempSync(path.join(tmpdir(), "ait-multi-window-"));
const fixture = path.join(temporary, "fixture.cjs");
let instance;

try {
  const renderer = await build({
    stdin: {
      contents: `
        import { z } from "zod";
        import { createValidatedPersistStorage } from "./apps/mobile/src/storage/validated-persist-storage";
        export function createStorage() {
          return createValidatedPersistStorage({
            getItem: async (key) => localStorage.getItem(key),
            setItem: async (key, value) => localStorage.setItem(key, value),
            removeItem: async (key) => localStorage.removeItem(key),
          }, z.strictObject({ records: z.record(z.string(), z.string()) }), {
            mergeRecordFields: ["records"], version: 1,
          });
        }
      `,
      resolveDir: root,
      loader: "ts",
    },
    bundle: true,
    write: false,
    format: "iife",
    globalName: "windowFixture",
    platform: "browser",
  });
  writeFileSync(path.join(temporary, "renderer.js"), renderer.outputFiles[0].text);
  writeFileSync(
    fixture,
    `
    const { app, BrowserWindow, ipcMain, protocol } = require("electron");
    const { readFileSync } = require("node:fs");
    const { createDesktopWindowOwner } = require(${JSON.stringify(path.join(desktop, "dist/window/desktop-window-owner.js"))});
    const { resolveNewWindowOptions } = require(${JSON.stringify(path.join(desktop, "dist/window/open-new-window.js"))});
    const { setupApplicationMenu } = require(${JSON.stringify(path.join(desktop, "dist/features/menu.js"))});
    app.setPath("userData", ${JSON.stringify(path.join(temporary, "profile"))});
    protocol.registerSchemesAsPrivileged([{ scheme: "ait", privileges: { standard: true, secure: true, supportFetchAPI: true } }]);
    const windows = new Map();
    function owned(win) {
      return {
        webContentsId: win.webContents.id,
        isDestroyed: () => win.isDestroyed(), isVisible: () => win.isVisible(),
        isMinimized: () => win.isMinimized(), restore: () => win.restore(),
        show: () => win.show(), focus: () => win.focus(), sendAgent: () => {},
      };
    }
    const owner = createDesktopWindowOwner({
      async create(input) {
        const win = new BrowserWindow({ show: false, webPreferences: {
          preload: ${JSON.stringify(path.join(desktop, "dist/preload.js"))},
          contextIsolation: true, nodeIntegration: false,
        } });
        const id = win.webContents.id;
        windows.set(id, win);
        input.onCreated(id);
        win.on("closed", () => { windows.delete(id); input.onClosed(id); });
        await win.loadURL("ait://app" + (input.initialRoute ?? "/"));
        return owned(win);
      },
      windows: () => [...windows.values()].map(owned), focusedWindow: () => null,
      agentRoute: () => "/", deliverAgent: () => null,
    });
    app.whenReady().then(async () => {
      protocol.handle("ait", (request) => new URL(request.url).pathname === "/renderer.js"
        ? new Response(readFileSync(${JSON.stringify(path.join(temporary, "renderer.js"))}), { headers: { "Content-Type": "application/javascript" } })
        : new Response('<!doctype html><title>Window fixture</title><script src="/renderer.js"></script>', { headers: { "Content-Type": "text/html" } }));
      ipcMain.handle("paseo:window:openNew", (_event, options) => owner.openAdditional(resolveNewWindowOptions(options)));
      ipcMain.handle("paseo:get-pending-open-project", (event) => owner.takePendingProject(event.sender.id));
      setupApplicationMenu({ onNewWindow: () => owner.openAdditional() });
      await owner.openPrimary();
    });
  `,
  );
  const env = { ...process.env };
  delete env.ELECTRON_RUN_AS_NODE;
  instance = await electron.launch({
    executablePath: require("electron"),
    args: [fixture],
    env,
    timeout: 30_000,
  });
  const first = await instance.firstWindow();
  await first.waitForFunction(() => typeof windowFixture !== "undefined");
  assert.equal(await first.evaluate(() => typeof navigator.locks?.request), "function");

  const newWindow = instance.waitForEvent("window");
  await instance.evaluate(({ Menu }) => {
    const menu = Menu.getApplicationMenu().items.find((item) => item.label === "File");
    menu.submenu.items.find((item) => item.label === "New Window").click();
  });
  const second = await newWindow;
  await second.waitForURL("ait://app/");
  await second.waitForFunction(() => typeof windowFixture !== "undefined");
  assert.equal(
    await instance.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().length),
    2,
  );

  const targetWindow = instance.waitForEvent("window");
  await first.evaluate(() =>
    paseoDesktop.window.openNew({
      workspace: { serverId: "remote-host", workspaceId: "workspace-a" },
    }),
  );
  const targeted = await targetWindow;
  await targeted.waitForURL("ait://app/h/remote-host/workspace/workspace-a");
  assert.equal(first.url(), "ait://app/");

  await first.evaluate(() =>
    localStorage.setItem("shared-state", JSON.stringify({ state: { records: {} }, version: 1 })),
  );
  for (const page of [first, second]) {
    await page.evaluate(async () => {
      window.stateStorage = windowFixture.createStorage();
      await window.stateStorage.getItem("shared-state");
    });
  }
  await Promise.all([
    first.evaluate(() =>
      window.stateStorage.setItem("shared-state", {
        state: { records: { a: "first-window" } },
        version: 1,
      }),
    ),
    second.evaluate(() =>
      window.stateStorage.setItem("shared-state", {
        state: { records: { b: "second-window" } },
        version: 1,
      }),
    ),
  ]);
  assert.deepEqual(
    await first.evaluate(() => JSON.parse(localStorage.getItem("shared-state")).state.records),
    { a: "first-window", b: "second-window" },
  );
  await first.evaluate(() =>
    window.stateStorage.setItem("shared-state", { state: { records: {} }, version: 1 }),
  );
  await second.evaluate(() =>
    window.stateStorage.setItem("shared-state", {
      state: { records: { b: "updated" } },
      version: 1,
    }),
  );
  assert.deepEqual(
    await second.evaluate(() => JSON.parse(localStorage.getItem("shared-state")).state.records),
    { b: "updated" },
  );
  const closed = first.waitForEvent("close");
  const nativeFirst = await instance.browserWindow(first);
  await nativeFirst.evaluate((win) => win.close());
  await closed;
  assert.equal(
    await instance.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().length),
    2,
  );
  assert.equal(await second.evaluate(() => document.title), "Window fixture");
  console.log(
    "PASS: native menu, independent Workspace window route, shared Web Locks, concurrent saves, deletion, closing one window",
  );
} finally {
  await instance?.close();
  rmSync(temporary, { recursive: true, force: true });
}

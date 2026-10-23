import assert from "node:assert/strict";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { _electron as electron } from "playwright";

assert.equal(process.platform, "win32", "Run the packaged Windows smoke test on Windows");
const desktop = fileURLToPath(new URL("..", import.meta.url));
const packaged = path.join(desktop, "release/win-unpacked");
assert(!existsSync(path.join(packaged, "resources/bin")), "Windows must not bundle a daemon");
const temporary = mkdtempSync(path.join(os.tmpdir(), "ait-windows-smoke-"));
const daemonHome = path.join(temporary, "daemon");
const env = {
  ...process.env,
  AIT_ELECTRON_USER_DATA_DIR: path.join(temporary, "electron"),
  AIT_SERVER_DATA_DIR: daemonHome,
};
delete env.ELECTRON_RUN_AS_NODE;
delete env.AIT_SERVER_BIN;
let app;
try {
  app = await electron.launch({
    executablePath: path.join(packaged, "Ait.exe"),
    env,
    timeout: 60000,
  });
  const page = await app.firstWindow();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.waitForFunction(() => typeof window.paseoDesktop?.invoke === "function");
  await page.waitForFunction(() => location.pathname.endsWith("/welcome"), null, {
    timeout: 60000,
  });
  const state = await page.evaluate(async () => ({
    platform: window.paseoDesktop.platform,
    daemon: await window.paseoDesktop.invoke("desktop_daemon_status"),
    settings: await window.paseoDesktop.invoke("get_desktop_settings"),
    account: await window.paseoDesktop.invoke("account_status"),
    hosts: globalThis.__paseoHostRuntimeStore.getHosts(),
  }));
  assert.equal(state.platform, "win32");
  assert.equal(state.daemon.status, "stopped");
  assert.equal(state.daemon.pid, null);
  assert.equal(state.settings.daemon.manageBuiltInDaemon, false);
  assert.equal(state.account.status, "logged_out");
  assert.deepEqual(state.hosts, []);
  await assert.rejects(
    page.evaluate(() => window.paseoDesktop.invoke("start_desktop_daemon")),
    /does not include a built-in daemon/,
  );
  assert(!existsSync(daemonHome), "Windows startup must not create daemon runtime data");
  assert.deepEqual(errors, [], "Packaged renderer must start without JavaScript errors");
  console.log(
    "Windows packaged startup passed: welcome screen, account bridge, no default host or daemon.",
  );
} finally {
  await app?.close();
  rmSync(temporary, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
}

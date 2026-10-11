// Keep the built-in daemon after quit, then reattach to the same process on the next launch.
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, rmSync, statSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { _electron as electron } from "playwright";

const require = createRequire(import.meta.url);
const desktop = fileURLToPath(new URL("..", import.meta.url));
const root = path.resolve(desktop, "../..");
const temporary = mkdtempSync(path.join(os.tmpdir(), "ait-desktop-keep-running-"));
const serverHome = path.join(temporary, "server");
const record = path.join(serverHome, "desktop-daemon.json");
const env = {
  ...process.env,
  AIT_SERVER_DATA_DIR: serverHome,
  AIT_SERVER_BIN:
    process.env.AIT_SERVER_BIN ||
    path.join(root, "target/debug", process.platform === "win32" ? "daemon.exe" : "daemon"),
  AIT_ELECTRON_USER_DATA_DIR: path.join(temporary, "electron"),
};
delete env.ELECTRON_RUN_AS_NODE;

function isAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

async function launch() {
  const app = await electron.launch({
    executablePath: require("electron"),
    args: [desktop],
    env,
    timeout: 60000,
  });
  const page = await app.firstWindow();
  await page.waitForFunction(() => typeof window.paseoDesktop?.invoke === "function");
  const deadline = Date.now() + 90_000;
  let status;
  do {
    status = await page.evaluate(() => window.paseoDesktop.invoke("desktop_daemon_status"));
    if (status.status === "running") break;
    if (status.status === "errored") throw new Error(status.error);
    await page.waitForTimeout(250);
  } while (Date.now() < deadline);
  assert.equal(status.status, "running", JSON.stringify(status));
  await page.waitForFunction(
    (id) => globalThis.__paseoHostRuntimeStore?.getSnapshot(id)?.connectionStatus === "online",
    status.serverId,
    { timeout: 30000 },
  );
  return { app, page, status };
}

let app;
let pid;
try {
  const first = await launch();
  app = first.app;
  pid = first.status.pid;
  await first.page.evaluate(() =>
    window.paseoDesktop.invoke("patch_desktop_settings", {
      daemon: { keepRunningAfterQuit: true },
    }),
  );
  await app.close();
  app = null;
  assert(isAlive(pid), "Daemon stopped although keep-running was enabled");
  assert(existsSync(record), "Daemon handover record was not written");
  assert.equal(statSync(record).mode & 0o077, 0, "Daemon handover record is readable by others");

  const second = await launch();
  app = second.app;
  assert.equal(second.status.pid, pid, "Next launch started a new daemon instead of reattaching");
  assert.equal(second.status.serverId, first.status.serverId);
  assert(second.status.ownedByDesktop);
  const projects = await second.page.evaluate(async (id) => {
    return await globalThis.__paseoHostRuntimeStore.getSnapshot(id).client.listProjects();
  }, second.status.serverId);
  assert(Array.isArray(projects.projects), "Reattached daemon did not accept renderer RPC");

  await second.page.evaluate(() =>
    window.paseoDesktop.invoke("patch_desktop_settings", {
      daemon: { keepRunningAfterQuit: false },
    }),
  );
  await app.close();
  app = null;
  const deadline = Date.now() + 20_000;
  while (isAlive(pid) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
  assert(!isAlive(pid), "Reattached daemon survived quit with keep-running disabled");
  assert(!existsSync(record), "Stale daemon handover record remained after stop");
  pid = null;
  console.log(JSON.stringify({ status: "passed" }));
} finally {
  await app?.close();
  if (pid && isAlive(pid)) process.kill(pid, "SIGTERM");
  rmSync(temporary, { recursive: true, force: true });
}

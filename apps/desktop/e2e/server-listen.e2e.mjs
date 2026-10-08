import assert from "node:assert/strict";
import { expect } from "@playwright/test";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { _electron as electron } from "playwright";

const require = createRequire(import.meta.url);
const desktop = fileURLToPath(new URL("..", import.meta.url));
const root = path.resolve(desktop, "../..");
const temporary = mkdtempSync(path.join(os.tmpdir(), "ait-server-listen-"));
const env = {
  ...process.env,
  EXPO_DEV_URL: process.env.EXPO_DEV_URL || "http://localhost:8082",
  AIT_TEST_APP_NAME: "Ait Listener Test",
  AIT_DISABLE_SINGLE_INSTANCE_LOCK: "1",
  AIT_SERVER_DATA_DIR: path.join(temporary, "server"),
  AIT_SERVER_BIN: process.env.AIT_SERVER_BIN || path.join(root, "target/debug/daemon"),
  AIT_ELECTRON_USER_DATA_DIR: path.join(temporary, "electron"),
};
delete env.ELECTRON_RUN_AS_NODE;
delete env.AIT_SERVER_LISTEN;
let app;

async function launch() {
  app = await electron.launch({
    executablePath: require("electron"),
    args: [desktop],
    env,
    timeout: 60000,
  });
  const page = await app.firstWindow();
  await page.waitForFunction(() => typeof window.paseoDesktop?.invoke === "function");
  await page.evaluate(() =>
    localStorage.setItem("@paseo:app-settings", JSON.stringify({ language: "en" })),
  );
  await expect
    .poll(
      async () =>
        page.evaluate(
          async () => (await window.paseoDesktop.invoke("desktop_daemon_status")).status,
        ),
      { timeout: 120000 },
    )
    .toBe("running");
  const status = await page.evaluate(() => window.paseoDesktop.invoke("desktop_daemon_status"));
  await page.waitForFunction(
    (id) => globalThis.__paseoHostRuntimeStore?.getSnapshot(id)?.connectionStatus === "online",
    status.serverId,
    { timeout: 30000 },
  );
  await page.getByTestId("sidebar-settings").click();
  await page
    .getByTestId("settings-sidebar")
    .filter({ visible: true })
    .getByRole("button", { name: "Overview", exact: true })
    .click();
  const daemon = page.getByTestId("host-page-daemon-lifecycle-card");
  await expect(daemon.getByTestId("server-listen-host")).toBeVisible();
  await expect(
    daemon.getByTestId("daemon-status-row").getByTestId("daemon-status-listen"),
  ).toHaveText(status.listen);
  await expect(page.getByTestId("server-listen-save")).toHaveCount(0);
  await expect(page.getByText("Restart daemon to apply changes.")).toBeVisible();
  await expect(page.getByText("Server listener", { exact: true })).toHaveCount(0);
  await expect(page.getByText(/Changes apply the next time the desktop/)).toHaveCount(0);
  return { page, status };
}

async function save(page, host, port) {
  const previous = await page.evaluate(() => window.paseoDesktop.invoke("desktop_daemon_status"));
  await page.getByTestId("server-listen-host").fill(host);
  await page.getByTestId("server-listen-port").fill(port);
  await page.getByTestId("server-listen-port").press("Tab");
  await expect
    .poll(() => {
      const config = JSON.parse(
        readFileSync(path.join(env.AIT_ELECTRON_USER_DATA_DIR, "desktop-settings.json"), "utf8"),
      );
      return config.settings.daemon.listen;
    })
    .toBe(`${host}:${port}`);
  assert.equal(
    (await page.evaluate(() => window.paseoDesktop.invoke("desktop_daemon_status"))).listen,
    previous.listen,
  );
  await app.evaluate(({ dialog }) => {
    dialog.showMessageBox = async () => ({ response: 1, checkboxChecked: false });
  });
  await page.getByTestId("host-page-restart-button").click();
  await expect
    .poll(
      async () => {
        const status = await page.evaluate(() =>
          window.paseoDesktop.invoke("desktop_daemon_status"),
        );
        return (
          status.status === "running" &&
          (port === "0"
            ? status.listen.startsWith(`${host}:`)
            : status.listen === `${host}:${port}`)
        );
      },
      { timeout: 60000 },
    )
    .toBe(true);
  const applied = await page.evaluate(() => window.paseoDesktop.invoke("desktop_daemon_status"));
  await expect(page.getByTestId("daemon-status-listen")).toHaveText(applied.listen);
  await page.waitForFunction(
    (id) => globalThis.__paseoHostRuntimeStore?.getSnapshot(id)?.connectionStatus === "online",
    applied.serverId,
    { timeout: 30000 },
  );
  return applied;
}

try {
  let { page, status } = await launch();
  const serverId = status.serverId;
  await expect(page.getByTestId("server-listen-host")).toHaveValue("127.0.0.1");
  await expect(page.getByTestId("server-listen-port")).toHaveValue("0");
  await page.getByTestId("server-listen-port").fill("65536");
  await page.getByTestId("server-listen-port").press("Tab");
  await expect(
    page.getByText("Enter an IPv4 or IPv6 address (or localhost) and a port from 0 to 65535."),
  ).toBeVisible();
  const applied = await save(page, "0.0.0.0", "0");
  assert.notEqual(applied.pid, status.pid);
  assert.equal(applied.serverId, serverId);
  await app.close();

  ({ page, status } = await launch());
  assert.equal(status.serverId, serverId);
  assert.match(status.listen, /^0\.0\.0\.0:\d+$/);
  assert.equal(status.connectAddress, status.listen.replace("0.0.0.0", "127.0.0.1"));
  await expect(page.getByTestId("server-listen-host")).toHaveValue("0.0.0.0");
  const port = status.listen.split(":").at(-1);
  await save(page, "127.0.0.1", port);
  await app.close();

  ({ page, status } = await launch());
  assert.equal(status.serverId, serverId);
  assert.equal(status.listen, `127.0.0.1:${port}`);
  await expect(page.getByTestId("server-listen-port")).toHaveValue(port);
  await page.screenshot({ path: path.join(os.tmpdir(), "ait-server-listen-overview.png") });
  console.log(
    "Listener settings saved on blur and applied by Restart daemon; wildcard and fixed-port reconnection passed across three Electron launches.",
  );
} finally {
  if (app) await app.close().catch(() => {});
  rmSync(temporary, { recursive: true, force: true });
}

import assert from "node:assert/strict";
import { expect } from "@playwright/test";
import { mkdtempSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { _electron as electron } from "playwright";

const require = createRequire(import.meta.url);
const desktop = fileURLToPath(new URL("..", import.meta.url));
const root = path.resolve(desktop, "../..");
const temporary = mkdtempSync(path.join(os.tmpdir(), "ait-paseo-desktop-smoke-"));
const packagedApp = process.env.AIT_PACKAGED_APP;
const env = {
  ...process.env,
  EXPO_DEV_URL: process.env.EXPO_DEV_URL || "http://localhost:8082",
  AIT_TEST_APP_NAME: "Ait Settings Audit",
  AIT_DISABLE_SINGLE_INSTANCE_LOCK: "1",
  AIT_SERVER_DATA_DIR: path.join(temporary, "server"),
  AIT_SERVER_BIN:
    process.env.AIT_SERVER_BIN ||
    path.join(root, "target/debug", process.platform === "win32" ? "daemon.exe" : "daemon"),
  AIT_ELECTRON_USER_DATA_DIR: path.join(temporary, "electron"),
};
if (packagedApp) delete env.AIT_SERVER_BIN;
delete env.ELECTRON_RUN_AS_NODE;
let app;
let page;
let pid;
try {
  app = await electron.launch({
    executablePath: packagedApp
      ? path.join(packagedApp, "Contents/MacOS/Ait")
      : require("electron"),
    args: packagedApp ? [] : [desktop],
    env,
    timeout: 60000,
  });
  page = await app.firstWindow();
  page.setDefaultTimeout(10000);
  await page.evaluate(() => {
    localStorage.setItem("@paseo:app-settings", JSON.stringify({ language: "en" }));
  });
  const errors = [];
  const rpcFailures = [];
  page.on("pageerror", (error) => {
    errors.push(error.message);
    console.error("renderer error:", error.message);
  });
  await page.waitForFunction(() => typeof window.paseoDesktop?.invoke === "function");
  // The renderer must bootstrap the daemon itself. This test never sends start.
  const deadline = Date.now() + 90_000;
  let status;
  do {
    status = await page.evaluate(() => window.paseoDesktop.invoke("desktop_daemon_status"));
    if (status.status === "running") break;
    if (status.status === "errored") throw new Error(status.error);
    await page.waitForTimeout(100);
  } while (Date.now() < deadline);
  assert.equal(status.status, "running", JSON.stringify(status));
  assert(status.serverId && status.ownedByDesktop && status.listen);
  assert(!("token" in status) && !("bearerToken" in status));
  pid = status.pid;
  await page.waitForFunction(
    (id) => globalThis.__paseoHostRuntimeStore?.getSnapshot(id)?.connectionStatus === "online",
    status.serverId,
    { timeout: 30000 },
  );

  page.on("console", (message) => {
    if (
      message.type() === "error" &&
      /Subscription failed|DaemonRpcError|No Rust server method mapping/.test(message.text())
    )
      rpcFailures.push(message.text());
  });
  await page.getByTestId("sidebar-settings").click();

  const sidebar = page.getByTestId("settings-sidebar").filter({ visible: true });
  const destinations = [
    "General",
    "Online Service",
    "Appearance",
    "Layout",
    "Editor",
    "Shortcuts",
    "Notifications",
    "Permissions",
    "Diagnostics",
    "About",
    "Overview",
    "Projects",
    "Connections",
    "Agents",
    "Metadata",
    "Workspaces",
    "Providers",
    "Usage",
    "Terminals",
  ];
  async function open(title) {
    console.log(`Checking settings: ${title}`);
    await sidebar.getByRole("button", { name: title, exact: true }).click();
    await expect(
      page.getByTestId("settings-detail-header-title").filter({ visible: true }),
    ).toHaveText(title);
  }
  await expect(sidebar.getByRole("button", { name: "Plugins", exact: true })).toHaveCount(0);
  for (const title of destinations) {
    await open(title);
    await expect(page.getByTestId("settings-detail-pane").filter({ visible: true })).not.toHaveText(
      title,
      { timeout: 30000 },
    );
  }

  await open("About");
  const changelogUrl = "https://raw.githubusercontent.com/ait-app/ait/main/CHANGELOG.md";
  await page.route(changelogUrl, (route) => route.fulfill({ status: 404, body: "Not found" }));
  const unavailableChangelog = page.waitForResponse(changelogUrl);
  await page.getByTestId("settings-whats-new").click();
  assert.equal((await unavailableChangelog).status(), 404);
  const changelog = page.getByTestId("changelog-sheet");
  await expect(changelog).toBeVisible();
  const bundledRelease = readFileSync(path.join(root, "CHANGELOG.md"), "utf8").match(
    /^## (\S+)[^\n]*\n+\s*- ([^\n]+)/m,
  );
  assert(bundledRelease, "The bundled changelog must contain a release and its notes");
  await expect(changelog.getByTestId(`changelog-release-${bundledRelease[1]}`)).toContainText(
    bundledRelease[2].replace(/`/g, ""),
  );
  await expect(changelog.getByTestId("changelog-error")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(changelog).not.toBeVisible();
  await page.unroute(changelogUrl);

  await open("Projects");
  await expect(page.getByTestId("projects-list")).toContainText("No projects yet");
  const projectRoot = path.join(temporary, "settings-audit-project");
  mkdirSync(projectRoot);
  await page.evaluate(
    async ({ id, directory }) => {
      const client = globalThis.__paseoHostRuntimeStore.getSnapshot(id).client;
      const result = await client.addProject(directory);
      if (result.error) throw new Error(result.error);
    },
    { id: status.serverId, directory: projectRoot },
  );
  await expect(page.getByTestId("projects-list")).toContainText("settings-audit-project", {
    timeout: 15000,
  });
  await page.getByTestId("projects-list").getByRole("button").first().click();
  await expect(page.getByTestId("project-edit-button")).toBeVisible();

  await open("Agents");
  await expect(page.getByTestId("agent-profiles-card")).toBeVisible();
  await page.getByTestId("agent-profiles-add-button").click();
  await expect(page.getByTestId("agent-profile-name-input")).toBeVisible();
  await page.getByTestId("agent-profile-cancel-button").click();
  const skills = page.getByTestId("host-agent-skills-card");
  await expect(skills.getByRole("button")).toBeEnabled({ timeout: 15000 });
  await skills.getByRole("button").click();
  await expect(page.getByTestId("skill-selection-sheet")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("skill-selection-sheet")).toHaveCount(0);
  await page.getByTestId("host-page-append-system-prompt-edit").click();
  await page.getByTestId("host-page-append-system-prompt-input").fill("Settings regression prompt");
  await page.getByTestId("host-page-append-system-prompt-save").click();
  await expect(page.getByTestId("host-page-append-system-prompt-sheet")).toHaveCount(0);
  const config = () =>
    page.evaluate(
      async (id) =>
        (await globalThis.__paseoHostRuntimeStore.getSnapshot(id).client.getDaemonConfig()).config,
      status.serverId,
    );
  assert.equal((await config()).appendSystemPrompt, "Settings regression prompt");

  await open("Metadata");
  await expect(page.getByTestId("metadata-generation-settings")).toBeVisible();
  await open("Workspaces");
  await page.getByTestId("host-page-auto-archive-merged-workspaces-switch").click();
  await expect.poll(async () => (await config()).autoArchiveAfterMerge).toBe(true);
  await open("Terminals");
  await page.getByTestId("terminal-profiles-add-button").click();
  await expect(page.getByTestId("terminal-profile-name-input")).toBeFocused();
  await page.getByTestId("terminal-profile-name-input").fill("Audit terminal");
  await page.getByTestId("terminal-profile-command-input").fill("echo");
  await page.getByTestId("terminal-profile-args-input").fill("hello");
  await expect(page.getByTestId("terminal-profile-command-input")).toHaveValue("echo");
  await page.getByTestId("terminal-profile-save-button").click();
  await expect(page.getByTestId("terminal-profiles-card")).toContainText("Audit terminal");
  assert((await config()).terminalProfiles.some((profile) => profile.name === "Audit terminal"));

  await open("Providers");
  await expect(page.getByTestId("host-page-providers-card")).toContainText(/codex/i, {
    timeout: 60000,
  });
  await page
    .getByTestId("host-page-providers-card")
    .getByRole("button", { name: /codex/i })
    .first()
    .click();
  await expect(page.getByTestId("provider-settings-sheet")).toBeVisible();
  await expect(page.getByTestId("host-page-add-provider-card")).toHaveCount(0);
  await expect(
    page
      .getByTestId("provider-settings-sheet")
      .getByRole("button", { name: "Add model", exact: true }),
  ).toHaveCount(0);
  await page.keyboard.press("Escape");
  assert.deepEqual(errors, []);
  assert.deepEqual(rpcFailures, []);
  console.log(
    JSON.stringify({
      status: "passed",
      destinations: destinations.length,
      checks: [
        "no plugins",
        "bundled changelog on HTTP 404",
        "project polling and details",
        "agent profile editor",
        "skills selection",
        "system prompt saved",
        "workspace toggle saved",
        "terminal profile saved",
        "provider settings opened",
      ],
      rendererErrors: errors,
    }),
  );
  await app.close();
  app = null;
  assert.throws(() => process.kill(pid, 0), "Rust child survived normal Electron quit");
  pid = null;
} catch (error) {
  if (page && !page.isClosed()) {
    console.error(
      "Renderer body:",
      await page
        .locator("body")
        .innerText()
        .catch(() => "closed"),
    );
    if (process.env.PASEO_SMOKE_SCREENSHOT)
      await page.screenshot({ path: process.env.PASEO_SMOKE_SCREENSHOT });
  }
  throw error;
} finally {
  await app?.close();
  if (pid) {
    try {
      process.kill(pid, "SIGTERM");
    } catch {}
  }
  rmSync(temporary, { recursive: true, force: true });
}

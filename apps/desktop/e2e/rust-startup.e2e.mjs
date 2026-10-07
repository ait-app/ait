import assert from "node:assert/strict";
import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { _electron as electron } from "playwright";

const require = createRequire(import.meta.url);
const desktop = fileURLToPath(new URL("..", import.meta.url));
const root = path.resolve(desktop, "../..");
const temporary = mkdtempSync(path.join(os.tmpdir(), "ait-desktop-smoke-"));
const fixtureBin = path.join(temporary, "bin");
const workspace = path.join(temporary, "workspace");
mkdirSync(fixtureBin);
mkdirSync(workspace);
copyFileSync(
  path.join(root, "crates/provider/tests/fixtures/codex_app_server.py"),
  path.join(fixtureBin, "codex"),
);
chmodSync(path.join(fixtureBin, "codex"), 0o755);
const packagedApp = process.env.AIT_PACKAGED_APP;
const env = {
  ...process.env,
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
      ? path.join(packagedApp, process.platform === "darwin" ? "Contents/MacOS/Ait" : "Ait")
      : require("electron"),
    args: packagedApp ? [] : [desktop],
    env,
    timeout: 60000,
  });
  page = await app.firstWindow();
  const errors = [];
  page.on("pageerror", (error) => {
    errors.push(error.message);
    console.error("renderer error:", error.message);
  });
  page.on("console", (message) => {
    if (/Message validation failed|Subscription request failed/.test(message.text())) {
      errors.push(message.text());
    }
  });
  await page.waitForFunction(() => typeof window.paseoDesktop?.invoke === "function");
  // The renderer must bootstrap the daemon itself. This test never sends start.
  const deadline = Date.now() + 90_000;
  let status;
  do {
    status = await page.evaluate(() => window.paseoDesktop.invoke("desktop_daemon_status"));
    if (status.status === "running") break;
    if (status.status === "errored") throw new Error(status.error);
    await page.waitForTimeout(250);
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
  const projects = await page.evaluate(async (id) => {
    const runtime = globalThis.__paseoHostRuntimeStore;
    return await runtime.getSnapshot(id).client.listProjects();
  }, status.serverId);
  assert(Array.isArray(projects.projects), "Authenticated renderer RPC did not return projects");
  const terminals = await page.evaluate(
    async ({ id, cwd }) => {
      const client = globalThis.__paseoHostRuntimeStore.getSnapshot(id).client;
      const subscription = client.observeTerminals({ cwd });
      try {
        const snapshot = await subscription.ready;
        return { ...snapshot, connected: client.isConnected };
      } finally {
        await subscription.release();
      }
    },
    { id: status.serverId, cwd: temporary },
  );
  assert(Array.isArray(terminals.terminals) && terminals.subscriptionId);
  assert(terminals.connected, "Terminal subscription disconnected the renderer");
  // Login-shell hydration has finished. The next server must use only our offline
  // peer for Codex, regardless of tools installed on the developer's machine.
  await app.evaluate((_electron, bin) => {
    process.env.PATH = `${bin}:${process.env.PATH}`;
  }, fixtureBin);
  const restarted = await page.evaluate(() => window.paseoDesktop.invoke("restart_desktop_daemon"));
  assert.equal(restarted.serverId, status.serverId);
  assert.equal(restarted.listen, status.listen);
  assert.notEqual(restarted.pid, pid);
  assert.throws(() => process.kill(pid, 0), "Previous Rust process survived restart");
  pid = restarted.pid;
  await page.waitForFunction(
    (id) => globalThis.__paseoHostRuntimeStore?.getSnapshot(id)?.connectionStatus === "online",
    status.serverId,
    { timeout: 30000 },
  );
  const afterRestart = await page.evaluate(async (id) => {
    return await globalThis.__paseoHostRuntimeStore.getSnapshot(id).client.listProjects();
  }, status.serverId);
  assert(Array.isArray(afterRestart.projects), "Renderer did not reconnect after Rust restart");

  const live = await page.evaluate(
    async ({ id, cwd }) => {
      const client = globalThis.__paseoHostRuntimeStore.getSnapshot(id).client;
      const project = await client.createProjectDirectory({
        parentPath: cwd,
        name: "first-chat",
      });
      if (!project.project || !project.directoryPath)
        throw new Error(project.error ?? "No new project");
      const prompt = "first conversation";
      const created = await client.createWorkspace({
        idempotencyKey: "first-chat",
        source: {
          kind: "directory",
          path: project.directoryPath,
          projectId: project.project.projectId,
        },
        firstAgentContext: { prompt, attachments: [] },
        agent: {
          config: { provider: "codex", cwd: project.directoryPath },
          initialPrompt: prompt,
        },
      });
      if (!created.workspace || !created.agent)
        throw new Error(created.error ?? "No first conversation");
      const agent = created.agent;
      await client.waitForFinish(agent.id, 5000);
      const timeline = client.observeTimeline([agent.id]);
      const events = [];
      timeline.subscribe({
        snapshot: () => {},
        update: (message) => events.push(message.type),
      });
      try {
        await timeline.ready;
        const initial = await client.fetchAgentTimeline(agent.id, { timeout: 5000 });
        await client.sendAgentMessage(agent.id, "stream");
        const running = await client.fetchAgents({});
        await client.sendAgentMessage(agent.id, "continue", {
          activeTurnBehavior: "steer",
        });
        const finished = await client.waitForFinish(agent.id, 5000);
        const page = await client.fetchAgentTimeline(agent.id, { timeout: 5000 });
        return {
          agentId: agent.id,
          cwd: project.directoryPath,
          initialEntries: initial.entries,
          running: running.entries.find((entry) => entry.agent.id === agent.id)?.agent.status,
          finished: finished.status,
          entries: page.entries,
          events,
        };
      } finally {
        await timeline.release();
      }
    },
    { id: status.serverId, cwd: workspace },
  );
  assert.equal(live.initialEntries.length, 2);
  assert.equal(live.initialEntries[0].item.text, "first conversation");
  assert.equal(live.initialEntries[1].item.text, "Echo: first conversation");
  assert.equal(live.running, "running");
  assert.equal(live.finished, "idle");
  assert(live.events.includes("agent.stream"), "No live timeline updates reached the SDK");
  const assistantText = (entries) =>
    entries
      .filter((entry) => entry.item.type === "assistant_message")
      .map((entry) => entry.item.text)
      .join("");
  assert.equal(assistantText(live.entries), "Echo: first conversationEcho: stream + continue");
  assert(live.entries.some((entry) => entry.item.type === "tool_call"));
  const nativeRequests = readFileSync(path.join(live.cwd, "native-requests.jsonl"), "utf8");
  assert(nativeRequests.includes('"method": "turn/steer"'), "Offline peer was not exercised");

  const reloaded = await page.evaluate(() => window.paseoDesktop.invoke("restart_desktop_daemon"));
  assert.throws(() => process.kill(pid, 0), "Previous Rust process survived second restart");
  pid = reloaded.pid;
  await page.waitForFunction(
    (id) => globalThis.__paseoHostRuntimeStore?.getSnapshot(id)?.connectionStatus === "online",
    status.serverId,
    { timeout: 30000 },
  );
  const persisted = await page.evaluate(
    async ({ id, agentId }) => {
      const client = globalThis.__paseoHostRuntimeStore.getSnapshot(id).client;
      const agents = await client.fetchAgents({});
      const timeline = await client.fetchAgentTimeline(agentId, { timeout: 5000 });
      return { count: agents.entries.length, entries: timeline.entries };
    },
    { id: status.serverId, agentId: live.agentId },
  );
  assert.equal(persisted.count, 1);
  // Cold history has canonical items; live pages can contain incremental deltas.
  assert.equal(assistantText(persisted.entries), assistantText(live.entries));
  assert.deepEqual(
    persisted.entries
      .filter((entry) => entry.item.type === "user_message")
      .map((entry) => entry.item.text),
    ["first conversation", "stream", "continue"],
  );
  assert(persisted.entries.some((entry) => entry.item.type === "tool_call"));

  await page.waitForTimeout(2000);
  assert.equal(errors.length, 0, errors.join("\n"));
  if (process.env.PASEO_SMOKE_SCREENSHOT)
    await page.screenshot({ path: process.env.PASEO_SMOKE_SCREENSHOT });
  console.log(
    JSON.stringify({
      status: "passed",
      serverId: status.serverId,
      listen: status.listen,
      rendererErrors: errors,
      title: await page.title(),
    }),
  );
  await app.close();
  app = null;
  assert.throws(() => process.kill(pid, 0), "Rust child survived normal Electron quit");
  pid = null;
} catch (error) {
  if (page && !page.isClosed()) {
    console.error("Renderer body:", (await page.locator("body").innerText()).slice(0, 5000));
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

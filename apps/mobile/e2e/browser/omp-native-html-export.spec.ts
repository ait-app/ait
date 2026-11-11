import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, mkdirSync } from "node:fs";
import { readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import type { AgentNativeHtmlExportResult } from "@ait/protocol/messages";
import { expect, test } from "../support/fixtures";
import { requireAitDaemon, repoRoot } from "../support/helpers/ait-daemon";
import { connectDaemonClient } from "../support/helpers/daemon-client-loader";
import { daemonWsRoutePattern, getE2EDaemonPort } from "../support/helpers/daemon-port";
import { seedWorkspace } from "../support/helpers/seed-client";
import { buildHostAgentDetailRoute } from "../../src/utils/host-routes";

// Official OMP native HTML export needs the real installed binary plus a loopback model so
// no external inference or credentials are used. Skipped when the official binary is absent.
// A stable module-level agent dir lets the worker-scoped daemon env and the test body agree.
const ompHome = mkdtempSync(path.join(tmpdir(), "omp-native-export-"));
const agentDir = path.join(ompHome, "agent");
mkdirSync(agentDir, { recursive: true });

// The daemon must launch the official OMP binary with an isolated agent directory so the
// loopback model fixture is used and no developer provider state is read.
test.use({
  e2eDaemonEnvironment: {
    AIT_SERVER_OMP_BIN: process.env.AIT_TEST_OMP_BIN ?? "omp",
    PI_CODING_AGENT_DIR: agentDir,
    OMP_PROFILE: "",
    PI_PROFILE: "",
  },
});

async function startLoopbackModel(dir: string): Promise<{ port: number; child: ChildProcess }> {
  const portFile = path.join(dir, "model-port");
  const child = spawn(
    process.execPath,
    [path.join(repoRoot, "apps/mobile/e2e/support/helpers/omp-loopback-model.mjs"), portFile],
    { stdio: "ignore" },
  );
  const { promise, resolve, reject } = Promise.withResolvers<number>();
  const timer = setTimeout(() => reject(new Error("loopback model did not start")), 10_000);
  const poll = setInterval(async () => {
    try {
      const value = Number(await readFile(portFile, "utf8"));
      if (value > 0) {
        clearTimeout(timer);
        clearInterval(poll);
        resolve(value);
      }
    } catch {
      // not ready yet
    }
  }, 50);
  child.once("exit", (code) => {
    clearTimeout(timer);
    clearInterval(poll);
    reject(new Error(`loopback model exited early (${code})`));
  });
  const port = await promise;
  return { port, child };
}

test("omp native html export downloads real native bytes during an active turn", async ({
  page,
}, testInfo) => {
  test.skip(!process.env.AIT_TEST_OMP_BIN, "requires official AIT_TEST_OMP_BIN");
  const model = await startLoopbackModel(ompHome);
  await writeFile(
    path.join(agentDir, "models.yml"),
    JSON.stringify({
      providers: {
        "ait-local-fixture": {
          baseUrl: `http://127.0.0.1:${model.port}/v1`,
          api: "openai-completions",
          auth: "none",
          models: [
            {
              id: "fixture",
              name: "Local fixture",
              reasoning: false,
              input: ["text"],
              contextWindow: 32768,
              maxTokens: 1024,
              cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
            },
          ],
        },
      },
    }),
  );
  const workspace = await seedWorkspace({ repoPrefix: "omp-native-export-" });
  const client = await connectDaemonClient<DaemonClient>({ clientIdPrefix: "omp-native-export" });
  const artifacts: AgentNativeHtmlExportResult[] = [];
  const failures: unknown[] = [];
  let generations = 0;
  let activeAgentId: string | null = null;
  try {
    const agent = await client.createAgent({
      provider: "omp",
      model: JSON.stringify(["ait-local-fixture", "fixture"]),
      cwd: workspace.repoPath,
      workspaceId: workspace.workspaceId,
      title: "Native export fixture",
    });
    activeAgentId = agent.id;
    // Seed a completed turn so the native journal is persisted before exporting.
    await client.sendMessage(agent.id, "Seed the native journal for export.");
    await client.waitForAgentUpsert(agent.id, (snapshot) => snapshot.status === "idle");
    // Start a long-running turn; the export must not interrupt it.
    await client.sendMessage(agent.id, "Hold this turn open while exporting.");
    await client.waitForAgentUpsert(agent.id, (snapshot) => snapshot.status === "running");
    await expect
      .poll(async () => (await (await fetch(`http://127.0.0.1:${model.port}/held`)).json()).active)
      .toBe(true);

    await page.routeWebSocket(daemonWsRoutePattern(), (ws) => {
      const server = ws.connectToServer();
      const pending = new Set<string>();
      ws.onMessage((message) => {
        const frame = JSON.parse(message.toString());
        if (frame.method === "agent.native_html.export.request") {
          generations += 1;
          pending.add(frame.request_id);
        }
        server.send(message);
      });
      server.onMessage((message) => {
        const frame = JSON.parse(message.toString());
        if (frame.type === "response" && pending.delete(frame.request_id)) {
          if (frame.result) artifacts.push(frame.result);
          else failures.push(frame.error);
        }
        ws.send(message);
      });
    });

    const host = requireAitDaemon(Number(getE2EDaemonPort()));
    await page.setViewportSize({ width: 1280, height: 1000 });
    await page.goto(buildHostAgentDetailRoute(host.serverId, agent.id, workspace.workspaceId));
    await page.getByTestId("context-window-meter").hover();
    const surface = page.getByTestId("native-html-export");
    await expect(surface).toBeVisible();
    // The factual risk warning is present (native format is not inert).
    await expect(page.getByTestId("native-html-export-risk")).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath("native-export-risk.png") });

    const downloadEvent = page.waitForEvent("download", { timeout: 20_000 }).catch(() => null);
    await page.getByTestId("native-html-export-download").click();
    await expect.poll(() => generations).toBe(1);
    await expect.poll(() => artifacts.length + failures.length).toBe(1);
    expect(failures).toEqual([]);
    const download = await downloadEvent;
    if (!download) throw new Error("No completed browser download for native export");
    expect(await download.failure()).toBeNull();
    const artifact = artifacts.at(-1)!;
    expect(artifact).toMatchObject({ agentId: agent.id, source: "omp_native" });
    expect(download.suggestedFilename()).toBe(artifact.fileName);
    const bytes = await readFile((await download.path())!);
    expect(bytes.byteLength).toBe(artifact.size);
    const text = bytes.toString("utf8");
    const embeddedJournal = text.match(/<script id="session-data"[^>]*>([^<]+)<\/script>/);
    expect(embeddedJournal).not.toBeNull();
    const journal = JSON.parse(Buffer.from(embeddedJournal![1], "base64").toString("utf8"));
    expect(journal.entries).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          type: "message",
          message: expect.objectContaining({
            role: "user",
            content: expect.arrayContaining([
              expect.objectContaining({
                type: "text",
                text: "Seed the native journal for export.",
              }),
            ]),
          }),
        }),
      ]),
    );
    // The main turn is still running: export did not interrupt or complete it.
    expect((await client.getAgentStatistics(agent.id)).status).toBe("running");
    await fetch(`http://127.0.0.1:${model.port}/release`, { method: "POST" });
    await client.waitForAgentUpsert(agent.id, (snapshot) => snapshot.status === "idle");
  } finally {
    try {
      await testInfo.attach("native-export", {
        body: JSON.stringify({
          generations,
          failures,
          receipts: artifacts.map(({ size }) => ({ size })),
        }),
        contentType: "application/json",
      });
      if (activeAgentId) await client.cancelAgent(activeAgentId).catch(() => undefined);
    } finally {
      try {
        await client.close();
      } finally {
        model.child.kill();
        await workspace.cleanup();
        await rm(ompHome, { recursive: true, force: true });
      }
    }
  }
});

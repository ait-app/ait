import { readFile } from "node:fs/promises";
import path from "node:path";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import type { AgentTimelineExportResult } from "@ait/protocol/messages";
import { expect, test } from "../support/fixtures";
import { requireAitDaemon, repoRoot } from "../support/helpers/ait-daemon";
import { connectDaemonClient } from "../support/helpers/daemon-client-loader";
import { daemonWsRoutePattern, getE2EDaemonPort } from "../support/helpers/daemon-port";
import { seedWorkspace } from "../support/helpers/seed-client";
import { buildHostAgentDetailRoute } from "../../src/utils/host-routes";

// The existing worker fixture starts an isolated daemon with no real provider credentials.
test.use({
  e2eDaemonEnvironment: {
    AIT_SERVER_CODEX_BIN: path.join(repoRoot, "crates/provider/tests/fixtures/codex_app_server.py"),
  },
});

for (const scenario of ["download", "compact-download", "expired", "cancel", "old-host"] as const) {
  test(`timeline export visible surface: ${scenario}`, async ({ page }, testInfo) => {
    const compact = scenario === "compact-download";
    const downloading = scenario === "download" || compact;
    const workspace = await seedWorkspace({ repoPrefix: "timeline-export-" });
    const client = await connectDaemonClient<DaemonClient>({ clientIdPrefix: "timeline-export" });
    const pending = new Set<string>();
    const artifacts: AgentTimelineExportResult[] = [];
    const failures: unknown[] = [];
    const transport: string[] = [];
    page.on("requestfailed", (request) => {
      if (request.url().includes("/api/files/download"))
        transport.push(`failed: ${request.failure()?.errorText}`);
    });
    page.on("response", (response) => {
      if (response.url().includes("/api/files/download"))
        transport.push(`HTTP ${response.status()}`);
    });
    let generations = 0;
    let cancellations = 0;
    let activeAgentId: string | null = null;
    try {
      const agent = await client.createAgent({
        provider: "codex",
        model: "offline-model",
        cwd: workspace.repoPath,
        workspaceId: workspace.workspaceId,
        title: "Export <fixture>",
      });
      activeAgentId = agent.id;
      // Seed a completed native tool/result turn before exporting an active turn.
      await client.sendMessage(agent.id, "command-actions");
      await client.waitForAgentUpsert(agent.id, (snapshot) => snapshot.status === "idle");
      await client.sendMessage(agent.id, '<script>alert("export")</script> & conversation');
      await client.waitForAgentUpsert(agent.id, (snapshot) => snapshot.status === "idle");
      if (downloading) {
        await client.sendMessage(agent.id, "hang");
        await client.waitForAgentUpsert(agent.id, (snapshot) => snapshot.status === "running");
      }
      const nativeBefore = await readFile(
        path.join(workspace.repoPath, "native-requests.jsonl"),
        "utf8",
      );
      await page.routeWebSocket(daemonWsRoutePattern(), (ws) => {
        const server = ws.connectToServer();
        ws.onMessage((message) => {
          const frame = JSON.parse(message.toString());
          if (frame.method === "agent.timeline.export.request") {
            generations += 1;
            pending.add(frame.request_id);
          }
          if (frame.method === "agent.timeline.export.cancel.request") cancellations += 1;
          server.send(message);
        });
        server.onMessage((message) => {
          const frame = JSON.parse(message.toString());
          if (frame.type === "response" && pending.delete(frame.request_id)) {
            if (frame.result) artifacts.push(frame.result);
            else failures.push(frame.error);
          }
          if (scenario === "old-host" && frame.type === "server_info") {
            const keep = (method: string) => method !== "agent.timeline.export.cancel.request";
            frame.info.implemented_capabilities = frame.info.implemented_capabilities.filter(keep);
            frame.negotiated_capabilities = frame.negotiated_capabilities.filter(keep);
            ws.send(JSON.stringify(frame));
          } else ws.send(message);
        });
      });
      const host = requireAitDaemon(Number(getE2EDaemonPort()));
      await page.setViewportSize(
        compact ? { width: 390, height: 844 } : { width: 1280, height: 1000 },
      );
      await page.goto(buildHostAgentDetailRoute(host.serverId, agent.id, workspace.workspaceId));
      if (compact) await page.getByTestId("context-window-meter").click();
      else await page.getByTestId("context-window-meter").hover();
      const surface = page.getByTestId("timeline-export");
      await expect(surface).toBeVisible();
      if (scenario === "old-host") {
        await expect(surface).toContainText("Export unavailable");
        await expect(page.getByTestId("timeline-export-download")).toHaveCount(0);
        expect(generations).toBe(0);
        return;
      }
      if (downloading) {
        for (const scope of ["conversation", "timeline"] as const) {
          for (const format of ["markdown", "html"] as const) {
            if (!compact) await page.getByTestId("context-window-meter").hover();
            await surface
              .getByRole("radio", {
                name: scope === "conversation" ? "Conversation only" : "Whole saved Timeline",
                exact: true,
              })
              .click();
            await surface
              .getByRole("radio", {
                name: format === "markdown" ? "Markdown (.md)" : "HTML (.html)",
                exact: true,
              })
              .click();
            const generation = generations + 1;
            const downloadEvent = page
              .waitForEvent("download", { timeout: 15_000 })
              .catch(() => null);
            await page.getByTestId("timeline-export-download").click();
            await expect.poll(() => generations).toBe(generation);
            await expect.poll(() => artifacts.length + failures.length).toBe(generation);
            expect(failures).toEqual([]);
            const download = await downloadEvent;
            if (!download)
              throw new Error(
                `No completed browser download: ${JSON.stringify({ generations, cancellations, failures, transport })}`,
              );
            expect(await download.failure()).toBeNull();
            const artifact = artifacts.at(-1)!;
            expect(artifact).toMatchObject({
              agentId: agent.id,
              format,
              scope,
              source: "persisted_timeline",
            });
            expect(download.suggestedFilename()).toBe(artifact.fileName);
            const bytes = await readFile((await download.path())!);
            expect(bytes.byteLength).toBe(artifact.size);
            const text = bytes.toString("utf8");
            expect(text).toContain("conversation");
            expect(text).toContain("Echo:");
            if (scope === "timeline") expect(text).toContain("offline output");
            else expect(text).not.toContain("offline output");
            if (format === "html") {
              expect(text).toContain("&lt;script&gt;");
              expect(text).not.toContain('<script>alert("export")</script>');
            }
            await expect(page.getByTestId("timeline-export-download")).toBeEnabled();
          }
        }
        expect(generations).toBe(4);
        expect(cancellations).toBe(0);
        expect((await client.getAgentStatistics(agent.id)).status).toBe("running");
      } else {
        let requested = false;
        const finish = Promise.withResolvers<void>();
        let downloaded = false;
        page.on("download", () => {
          downloaded = true;
        });
        await page.route("**/api/files/download?token=*", async (route) => {
          requested = true;
          if (scenario === "cancel") await finish.promise;
          await route.fulfill({ status: 410, body: "Export expired" }).catch(() => undefined);
        });
        try {
          await page.getByTestId("timeline-export-download").click();
          await expect.poll(() => generations, { message: "Export action dispatch" }).toBe(1);
          await expect.poll(() => artifacts.length + failures.length).toBe(1);
          expect(failures).toEqual([]);
          await expect
            .poll(() => requested, { message: "Receipt must reach the HTTP transfer" })
            .toBe(true);
          if (scenario === "cancel") {
            await page.getByRole("button", { name: "Cancel export", exact: true }).click();
            finish.resolve();
          }
          await expect.poll(() => cancellations).toBe(1);
          expect(generations).toBe(1);
          expect(downloaded).toBe(false);
          expect(await client.cancelAgentTimelineExport(agent.id, artifacts[0]!.token)).toEqual({
            released: false,
          });
        } finally {
          finish.resolve();
        }
      }
      // Export never starts, resumes, interrupts or prompts the provider.
      const writes = (log: string) =>
        log
          .trim()
          .split("\n")
          .map((line) => JSON.parse(line))
          .filter((request) =>
            [
              "thread/start",
              "thread/resume",
              "turn/start",
              "turn/steer",
              "turn/interrupt",
            ].includes(request.method),
          );
      expect(
        writes(await readFile(path.join(workspace.repoPath, "native-requests.jsonl"), "utf8")),
      ).toEqual(writes(nativeBefore));
    } finally {
      try {
        await testInfo.attach("export-download", {
          body: JSON.stringify({
            generations,
            cancellations,
            failures,
            transport,
            receipts: artifacts.map(({ format, scope, size }) => ({ format, scope, size })),
          }),
          contentType: "application/json",
        });
        if (activeAgentId) await client.cancelAgent(activeAgentId);
      } finally {
        try {
          await client.close();
        } finally {
          await workspace.cleanup();
        }
      }
    }
  });
}

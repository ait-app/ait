import { readFile } from "node:fs/promises";
import path from "node:path";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import type { AgentSessionStatistics } from "@ait/protocol/messages";
import { expect, test } from "../support/fixtures";
import { requireAitDaemon, repoRoot } from "../support/helpers/ait-daemon";
import { connectDaemonClient } from "../support/helpers/daemon-client-loader";
import { daemonWsRoutePattern, getE2EDaemonPort } from "../support/helpers/daemon-port";
import { seedWorkspace } from "../support/helpers/seed-client";
import { buildHostAgentDetailRoute } from "../../src/utils/host-routes";

test.use({
  e2eDaemonEnvironment: {
    AIT_SERVER_CODEX_BIN: path.join(repoRoot, "crates/provider/tests/fixtures/codex_app_server.py"),
  },
});

for (const supported of [true, false]) {
  test(`session statistics visible entry on ${supported ? "supported" : "old"} host`, async ({
    page,
  }) => {
    const workspace = await seedWorkspace({ repoPrefix: "session-statistics-" });
    const client = await connectDaemonClient<DaemonClient>({
      clientIdPrefix: "session-statistics",
    });
    let requests = 0;
    try {
      const agent = await client.createAgent({
        provider: "codex",
        model: "offline-model",
        cwd: workspace.repoPath,
        workspaceId: workspace.workspaceId,
        title: "Statistics fixture",
      });
      const snapshot: AgentSessionStatistics = {
        agentId: agent.id,
        provider: "codex",
        status: "idle",
        configuredModel: "configured-fixture",
        effectiveModel: "effective-fixture",
        updatedAt: "2026-11-11T00:00:00Z",
        usageScope: "provider_snapshot",
        usage: {
          inputTokens: 12,
          cachedInputTokens: null,
          outputTokens: 0,
          totalCostUsd: 0,
          contextWindowMaxTokens: null,
          contextWindowUsedTokens: 0,
        },
      };
      await page.routeWebSocket(daemonWsRoutePattern(), (ws) => {
        const server = ws.connectToServer();
        ws.onMessage((message) => {
          const frame = JSON.parse(message.toString());
          if (frame.type === "request" && frame.method === "agent.statistics.get.request") {
            requests += 1;
            expect(frame.params.agentId).toBe(agent.id);
            ws.send(
              JSON.stringify({
                type: "response",
                request_id: frame.request_id,
                result: {
                  ...snapshot,
                  usage: { ...snapshot.usage, inputTokens: requests === 1 ? 12 : 7 },
                },
              }),
            );
          } else server.send(message);
        });
        server.onMessage((message) => {
          const frame = JSON.parse(message.toString());
          if (frame.type === "server_info") {
            const method = "agent.statistics.get.request";
            frame.info.implemented_capabilities = frame.info.implemented_capabilities.filter(
              (entry: string) => entry !== method,
            );
            frame.negotiated_capabilities = frame.negotiated_capabilities.filter(
              (entry: string) => entry !== method,
            );
            if (supported) {
              frame.info.implemented_capabilities.push(method);
              frame.negotiated_capabilities.push(method);
            }
            ws.send(JSON.stringify(frame));
          } else ws.send(message);
        });
      });
      const server = requireAitDaemon(Number(getE2EDaemonPort()));
      await page.setViewportSize({ width: 1280, height: 900 });
      await page.goto(buildHostAgentDetailRoute(server.serverId, agent.id, workspace.workspaceId));
      const meter = page.getByTestId("context-window-meter");
      await expect(meter).toBeVisible({ timeout: 30_000 });
      expect(requests).toBe(0);
      await meter.hover();
      await expect(page.getByText("Selected session statistics", { exact: true })).toBeVisible();
      if (!supported) {
        await expect(page.getByTestId("session-statistics-state")).toContainText("unavailable");
        expect(requests).toBe(0);
      } else {
        await expect(page.getByTestId("session-statistics-inputTokens")).toHaveText(
          "Input tokens: 12",
        );
        await expect(page.getByTestId("session-statistics-cachedInputTokens")).toHaveText(
          "Cached input tokens: Unavailable",
        );
        await expect(page.getByTestId("session-statistics-totalCostUsd")).toHaveText(
          "Reported cost (USD): 0",
        );
        await expect(page.getByTestId("session-statistics-configuredModel")).toContainText(
          "configured-fixture",
        );
        await expect(page.getByTestId("session-statistics-effectiveModel")).toContainText(
          "effective-fixture",
        );
        await page.mouse.move(0, 0);
        await expect(page.getByTestId("session-statistics")).toHaveCount(0);
        await meter.hover();
        await expect(page.getByTestId("session-statistics-inputTokens")).toHaveText(
          "Input tokens: 7",
        );
        expect(requests).toBe(2);
      }
    } finally {
      await client.close();
      await workspace.cleanup();
    }
  });
}

test("real daemon statistics remain read-only during an active native turn", async ({ page }) => {
  const workspace = await seedWorkspace({ repoPrefix: "session-statistics-active-" });
  const client = await connectDaemonClient<DaemonClient>({ clientIdPrefix: "statistics-active" });
  let agentId: string | undefined;
  try {
    const agent = await client.createAgent({
      provider: "codex",
      model: "offline-model",
      cwd: workspace.repoPath,
      workspaceId: workspace.workspaceId,
      title: "Active statistics fixture",
    });
    agentId = agent.id;
    await client.sendMessage(agent.id, "hang");
    const nativeRequests = () =>
      readFile(path.join(workspace.repoPath, "native-requests.jsonl"), "utf8");
    const before = await nativeRequests();
    for (let read = 0; read < 3; read += 1) {
      const statistics = await client.getAgentStatistics(agent.id);
      expect(statistics.status).toBe("running");
      expect(statistics.usageScope).toBe("provider_snapshot");
      expect(statistics.effectiveModel).toBe("offline-model");
    }
    expect(await nativeRequests()).toBe(before);
    const server = requireAitDaemon(Number(getE2EDaemonPort()));
    await page.goto(buildHostAgentDetailRoute(server.serverId, agent.id, workspace.workspaceId));
    await page.getByTestId("context-window-meter").hover();
    await expect(page.getByTestId("session-statistics-status")).toHaveText(
      "Execution status: Running",
    );
    await expect(page.getByTestId("session-statistics-effectiveModel")).toHaveText(
      "Actual model: offline-model",
    );
    expect((await client.getAgentStatistics(agent.id)).status).toBe("running");
    const writes = (log: string) =>
      log
        .trim()
        .split("\n")
        .map((line) => JSON.parse(line))
        .filter((request) =>
          ["thread/start", "thread/resume", "turn/start", "turn/steer", "turn/interrupt"].includes(
            request.method,
          ),
        );
    expect(writes(await nativeRequests())).toEqual(writes(before));
  } finally {
    if (agentId) await client.cancelAgent(agentId);
    await client.close();
    await workspace.cleanup();
  }
});

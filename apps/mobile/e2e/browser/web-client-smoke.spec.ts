import { expect, test } from "../support/fixtures";
import { submitMessage } from "../support/helpers/composer";
import { openAgentRoute } from "../support/helpers/mock-agent";
import { seedWorkspace } from "../support/helpers/seed-client";
import { getServerId } from "../support/helpers/server-id";

// CI's web-client smoke: one user path through the browser, a real daemon, and the
// offline Codex fixture. Keep it short; behavior coverage belongs in focused specs.
test("the web client sends a prompt and shows the provider reply", async ({ page }) => {
  // Each navigation parses the unminified Metro bundle, which is slow on CI runners.
  test.setTimeout(120_000);
  const workspace = await seedWorkspace({ repoPrefix: "ait-web-smoke-" });
  try {
    const agent = await workspace.client.createAgent({
      provider: "codex",
      model: "offline-model",
      cwd: workspace.repoPath,
      workspaceId: workspace.workspaceId,
      title: "Web client smoke",
    });

    await page.goto("/");
    await expect(
      page.getByTestId(`sidebar-workspace-row-${getServerId()}:${workspace.workspaceId}`),
    ).toBeVisible({ timeout: 30_000 });

    await openAgentRoute(page, { workspaceId: workspace.workspaceId, agentId: agent.id });
    await submitMessage(page, "Web client smoke prompt");
    await expect(page.getByText("Echo: Web client smoke prompt", { exact: true })).toBeVisible({
      timeout: 30_000,
    });
  } finally {
    await workspace.cleanup();
  }
});

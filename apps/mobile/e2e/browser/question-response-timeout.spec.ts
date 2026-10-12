import { expect, test } from "../support/fixtures";
import { daemonWsRoutePattern } from "../support/helpers/daemon-port";
import { openAgentRoute } from "../support/helpers/mock-agent";
import { seedWorkspace } from "../support/helpers/seed-client";

for (const action of ["dismiss", "submit"] as const) {
  test(`question ${action} recovers after a dropped request and can be retried`, async ({
    page,
  }) => {
    const workspace = await seedWorkspace({ repoPrefix: `question-${action}-timeout-` });
    try {
      const agent = await workspace.client.createAgent({
        provider: "codex",
        model: "offline-model",
        cwd: workspace.workspaceDirectory,
        workspaceId: workspace.workspaceId,
        title: "Question timeout",
        initialPrompt: "permit-question",
      });
      let attempts = 0;
      let droppedAt = 0;
      await page.routeWebSocket(daemonWsRoutePattern(), (browser) => {
        const server = browser.connectToServer();
        browser.onMessage((message) => {
          const envelope = JSON.parse(
            typeof message === "string" ? message : message.toString("utf8"),
          ) as {
            type?: string;
            method?: string;
            params?: { agentId?: string };
          };
          if (
            envelope.type === "request" &&
            envelope.method === "agent.permission.resolve.request" &&
            envelope.params?.agentId === agent.id
          ) {
            attempts += 1;
            if (attempts === 1) {
              droppedAt = Date.now();
              return;
            }
          }
          server.send(message);
        });
        server.onMessage((message) => browser.send(message));
      });
      await openAgentRoute(page, { agentId: agent.id, workspaceId: workspace.workspaceId });
      const card = page.getByTestId("question-form-card");
      await expect(card).toBeVisible();
      const option = card.getByRole("radio", { name: "first", exact: true });
      await option.click();
      const button = card.getByTestId(
        action === "dismiss" ? "question-form-dismiss" : "question-form-primary-action",
      );
      const label = action === "dismiss" ? "Dismiss" : "Submit";
      await button.click();
      await expect.poll(() => attempts).toBe(1);
      await expect(button).toBeDisabled();
      await expect(button).not.toContainText(label);
      await expect(button).toBeEnabled({ timeout: 7_000 });
      const elapsed = Date.now() - droppedAt;
      expect(elapsed).toBeGreaterThanOrEqual(4_500);
      expect(elapsed).toBeLessThan(7_500);
      await expect(button).toContainText(label);
      await expect(card).toBeVisible();
      await expect(option).toHaveAttribute("aria-checked", "true");
      await page.screenshot({
        path: `/tmp/question-${action}-timeout-recovered.png`,
        fullPage: true,
      });
      await button.click();
      await expect.poll(() => attempts).toBe(2);
      await expect(card).toHaveCount(0);
    } finally {
      await workspace.cleanup();
    }
  });
}

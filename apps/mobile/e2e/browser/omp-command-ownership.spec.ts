import { writeFile } from "node:fs/promises";
import path from "node:path";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { expect } from "../support/fixtures";
import { test } from "../support/helpers/omp-test-peer";
import { connectDaemonClient } from "../support/helpers/daemon-client-loader";
import { seedWorkspace } from "../support/helpers/seed-client";
import { openAgentRoute } from "../support/helpers/mock-agent";
import { composerLocator, expectComposerVisible, submitMessage } from "../support/helpers/composer";

test("native slash ownership keeps the agent and protects an unavailable session", async ({
  page,
  ompAgentDirectory,
}) => {
  test.setTimeout(120_000);
  await writeFile(
    path.join(ompAgentDirectory, "plugins.json"),
    JSON.stringify([
      { name: "clear", source: "extension", description: "Clear extension question" },
      { name: "nativecheck", source: "extension", description: "Native command question" },
    ]),
  );
  const workspace = await seedWorkspace({ repoPrefix: "omp-command-ownership-" });
  const client = await connectDaemonClient<DaemonClient>({ clientIdPrefix: "command-ownership" });
  try {
    const agent = await client.createAgent({
      provider: "omp",
      cwd: workspace.repoPath,
      workspaceId: workspace.workspaceId,
      model: JSON.stringify(["test", "model"]),
      title: "Native command ownership",
    });
    await openAgentRoute(page, { workspaceId: workspace.workspaceId, agentId: agent.id });
    await expectComposerVisible(page);
    await submitMessage(page, "Initialize the offline session");
    await expect(page.getByText("Hello world", { exact: true })).toBeVisible({ timeout: 30_000 });

    const input = composerLocator(page);
    const send = page.getByRole("button", { name: "Send message", exact: true });
    for (const [command, question] of [
      ["/clear", "Clear extension question"],
      ["/nativecheck", "Native command question"],
    ]) {
      await expect(input).toBeEditable();
      await input.fill(command);
      await send.click();
      await expect(page.getByTestId("question-form-current-question")).toHaveText(question, {
        timeout: 30_000,
      });
      expect((await client.fetchAgent(agent.id))?.agent.archivedAt ?? null).toBeNull();
      await page.getByText("A, B", { exact: true }).click();
      await page.getByTestId("question-form-primary-action").click();
      await expect(page.getByTestId("question-form-current-question")).not.toBeVisible();
    }

    await client.refreshAgent(agent.id);
    await expect(input).toBeEditable();
    await input.fill("/exit");
    await send.click();
    await expect(page.getByText(/session command catalog is not ready/i)).toBeVisible();
    await expect(input).toHaveValue("/exit");
    expect((await client.fetchAgent(agent.id))?.agent.archivedAt ?? null).toBeNull();
  } finally {
    await client.close();
    await workspace.cleanup();
  }
});

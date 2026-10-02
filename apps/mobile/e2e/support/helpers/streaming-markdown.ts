import { expect, type Page, type TestInfo } from "@playwright/test";
import { openAgentRoute, type MockAgentWorkspace } from "./mock-agent";
import { seedOfflineAgentWorkspace } from "./offline-agent";
import { writeFile } from "node:fs/promises";
import path from "node:path";

interface StreamingMarkdownAgent extends MockAgentWorkspace {
  advance(stage: number): Promise<void>;
}

export async function withStreamingMarkdown(
  page: Page,
  testInfo: TestInfo,
  run: (agent: StreamingMarkdownAgent) => Promise<void>,
): Promise<void> {
  testInfo.setTimeout(120_000);
  const agent = await seedOfflineAgentWorkspace({
    repoPrefix: "streaming-markdown-",
    title: "Streaming Markdown",
  });
  await writeFile(
    path.join(agent.cwd, "e2e-markdown-response.txt"),
    "**Bold text stays bold** and [Paseo docs](https://example.com/documentation). Done.",
  );
  const advance = (stage: number) =>
    writeFile(path.join(agent.cwd, `e2e-markdown-stage-${stage}`), "ready");
  try {
    await openAgentRoute(page, agent);
    await expect(page.getByTestId("message-input-root").filter({ visible: true })).toBeVisible();
    await run({ ...agent, advance });
  } finally {
    await advance(1);
    await advance(2);
    await agent.cleanup();
  }
}

export async function requestStreamingMarkdown(agent: StreamingMarkdownAgent): Promise<void> {
  await agent.client.sendAgentMessage(agent.agentId, "e2e-markdown-stream");
  // The offline producer pauses at actual unfinished Markdown boundaries. This
  // prevents history catch-up from racing a completed producer and makes the
  // browser consume genuine live deltas, not synthetic response snapshots.
}

export async function expectUnfinishedBold(page: Page): Promise<void> {
  const message = page.getByTestId("assistant-message").last();
  const bold = message.locator('[data-paseo-markdown-tag="strong"]');
  await expect(bold).toContainText("Bold");
  await expect(message).not.toContainText("stays bold");
  await expect(bold).toHaveCSS("font-weight", "500");
  await expect(message).not.toContainText("*");
}

export async function expectUnfinishedLink(
  page: Page,
  agent: StreamingMarkdownAgent,
  testInfo: TestInfo,
): Promise<void> {
  const message = page.getByTestId("assistant-message").last();
  await agent.advance(1);
  await expect(message).toContainText("Paseo docs");
  await expect(message.getByRole("link", { name: "Paseo docs" })).toHaveCount(0);
  await expect(message).not.toContainText("[");
  await expect(message).not.toContainText("https:");
  await captureMarkdown(page, testInfo, "unfinished-link");
}

export async function expectFinishedMarkdown(
  page: Page,
  agent: StreamingMarkdownAgent,
  testInfo: TestInfo,
): Promise<void> {
  await agent.advance(2);
  await agent.client.waitForFinish(agent.agentId, 30_000);
  await expectCompletedMarkdown(page);
  await captureMarkdown(page, testInfo, "completed-markdown");
}

export async function expectReloadedMarkdown(page: Page): Promise<void> {
  await page.reload({ waitUntil: "domcontentloaded" });
  await expectCompletedMarkdown(page);
}

async function captureMarkdown(page: Page, testInfo: TestInfo, name: string): Promise<void> {
  await testInfo.attach(name, {
    body: await page.screenshot({ path: testInfo.outputPath(`${name}.png`) }),
    contentType: "image/png",
  });
}

async function expectCompletedMarkdown(page: Page): Promise<void> {
  const message = page.getByTestId("assistant-message").last();
  await expect(message).toHaveText("Bold text stays bold and Paseo docs. Done.");
  await expect(
    message.getByRole("link", { name: "Paseo docs" }).and(message.locator("a")),
  ).toHaveAttribute("href", "https://example.com/documentation");
  await expect(message.locator('[data-paseo-markdown-tag="strong"]')).toHaveCSS(
    "font-weight",
    "500",
  );
}

import { access, writeFile } from "node:fs/promises";
import path from "node:path";
import { expect, type Page } from "../support/fixtures";
import { test } from "../support/helpers/omp-test-peer";
import { seedWorkspace } from "../support/helpers/seed-client";
import { clickNewChat, gotoWorkspace } from "../support/helpers/launcher";
import { expectComposerVisible } from "../support/helpers/composer";

test.use({ actionTimeout: 15_000 });

async function selectProviderModel(page: Page, provider: string, model: string) {
  await page.getByTestId("combined-model-selector").filter({ visible: true }).first().click();
  const providerRow = page.getByTestId(`model-provider-${provider}`);
  if (!(await providerRow.isVisible().catch(() => false))) {
    const back = page.getByTestId("sheet-header-back").filter({ visible: true }).last();
    if (await back.isVisible().catch(() => false)) await back.click();
  }
  if (await providerRow.isVisible().catch(() => false)) await providerRow.click();
  await page.getByTestId(`model-row-${provider}-${model}`).click();
  await expect(page.getByTestId("combobox-desktop-container")).not.toBeVisible();
}

test("a workspace draft remains editable and responds to resize while switching to slow Codex discovery", async ({
  page,
}) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: 1440, height: 1000 });
  const workspace = await seedWorkspace({ repoPrefix: "draft-codex-switch-" });
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  try {
    await writeFile(
      path.join(workspace.repoPath, "speed-models.json"),
      JSON.stringify({
        data: ["offline-model", "offline-alt"].map((model) => ({
          id: model,
          model,
          displayName: model,
          isDefault: model === "offline-model",
          hidden: false,
          supportedReasoningEfforts: [{ reasoningEffort: "high", description: "High effort" }],
          defaultReasoningEffort: "high",
          serviceTiers: [{ id: "fast" }],
        })),
        nextCursor: null,
      }),
    );
    await gotoWorkspace(page, workspace.workspaceId);
    await clickNewChat(page);
    await expectComposerVisible(page);
    await selectProviderModel(page, "omp", JSON.stringify(["test", "model"]));
    await writeFile(path.join(workspace.repoPath, "behavior"), "delayed-discovery");
    // This model has never been selected, so its feature request cannot reuse
    // the initial draft's cached default-model discovery.
    await selectProviderModel(page, "codex", "offline-alt");
    await expect
      .poll(async () => {
        try {
          await access(path.join(workspace.repoPath, "discovery-entered"));
          return true;
        } catch {
          return false;
        }
      })
      .toBe(true);
    const input = page.getByRole("textbox", { name: "Message agent..." }).first();
    await input.fill("Codex provider switch is responsive");
    const before = await input.boundingBox();
    await page.setViewportSize({ width: 1100, height: 800 });
    await expect(input).toHaveValue("Codex provider switch is responsive");
    await expect
      .poll(async () => (await input.boundingBox())?.width ?? Infinity)
      .toBeLessThan(before?.width ?? 0);
    await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(resolve)));
    await writeFile(path.join(workspace.repoPath, "release-discovery"), "");
    await page.setViewportSize({ width: 1440, height: 1000 });
    await expect(page.getByTestId("agent-feature-service_tier")).toBeVisible();
    await page.getByTestId("agent-feature-service_tier").click();
    await selectProviderModel(page, "omp", JSON.stringify(["test", "model"]));
    await input.fill("Still responsive after discovery");
    expect(errors).toEqual([]);
  } finally {
    await writeFile(path.join(workspace.repoPath, "release-discovery"), "");
    await workspace.cleanup();
  }
});

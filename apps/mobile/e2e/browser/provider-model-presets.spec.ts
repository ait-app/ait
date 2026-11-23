import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { expect, type Page } from "../support/fixtures";
import { test } from "../support/helpers/omp-test-peer";
import { connectDaemonClient } from "../support/helpers/daemon-client-loader";
import { seedWorkspace } from "../support/helpers/seed-client";
import { openAgentRoute } from "../support/helpers/mock-agent";
import { expectComposerVisible, submitMessage } from "../support/helpers/composer";
import { clickNewChat, gotoWorkspace } from "../support/helpers/launcher";

test.use({ actionTimeout: 15_000 });

async function openProviderModelCard(page: Page, mobile: boolean) {
  if (await page.getByTestId("selector-header-settings-omp").isVisible()) return;
  if (mobile) {
    await page.getByRole("button", { name: /Select model/ }).click();
    await page.getByTestId("agent-controls-model").click();
  } else {
    await page.getByTestId("combined-model-selector").filter({ visible: true }).first().click();
  }
  const provider = page.getByTestId("model-provider-omp");
  const settings = page.getByTestId("selector-header-settings-omp");
  const back = page.getByTestId("sheet-header-back").filter({ visible: true }).last();
  await expect(provider.or(settings).or(back)).toBeVisible();
  if (!(await settings.isVisible()) && (await back.isVisible())) await back.click();
  await expect(provider.or(settings)).toBeVisible();
  if (await provider.isVisible().catch(() => false)) await provider.click();
  await expect(page.getByTestId("selector-header-settings-omp")).toBeVisible();
}

test("same-model presets stay distinct across selection, editing and refresh", async ({
  page,
  ompAgentDirectory,
}) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: 1440, height: 1000 });
  const workspace = await seedWorkspace({ repoPrefix: "preset-identity-" });
  const client = await connectDaemonClient<DaemonClient>({ clientIdPrefix: "preset-identity" });
  const model = JSON.stringify(["test", "model"]);
  const first = "IdentityFirst";
  const second = "IdentitySecond";
  try {
    for (const name of [first, second]) {
      const saved = await client.saveProviderPreset(
        "omp",
        { name, model, thinkingOptionId: "low", featureValues: {} },
        workspace.repoPath,
      );
      expect(saved.error).toBeNull();
    }
    const agent = await client.createAgent({
      provider: "omp",
      cwd: workspace.repoPath,
      workspaceId: workspace.workspaceId,
      model,
      thinkingOptionId: "low",
      title: "Preset identity browser verification",
    });
    await openAgentRoute(page, { workspaceId: workspace.workspaceId, agentId: agent.id });
    await expectComposerVisible(page);

    const firstRow = page.getByTestId(`provider-model-preset-omp-${first}`);
    const secondRow = page.getByTestId(`provider-model-preset-omp-${second}`);
    const defaultRow = page.getByTestId(`model-row-omp-${model}`);
    await openProviderModelCard(page, false);
    await firstRow.click();
    await openProviderModelCard(page, false);
    await expect(firstRow).toHaveAttribute("aria-selected", "true");
    await expect(secondRow).toHaveAttribute("aria-selected", "false");
    await expect(defaultRow).toHaveAttribute("aria-selected", "false");

    await secondRow.click();
    await openProviderModelCard(page, false);
    await expect(firstRow).toHaveAttribute("aria-selected", "false");
    await expect(secondRow).toHaveAttribute("aria-selected", "true");
    await expect(defaultRow).toHaveAttribute("aria-selected", "false");
    await page.reload();
    await expectComposerVisible(page);
    await openProviderModelCard(page, false);
    await expect(secondRow).toHaveAttribute("aria-selected", "true");
    await expect(defaultRow).toHaveAttribute("aria-selected", "false");

    await defaultRow.click();
    await openProviderModelCard(page, false);
    await expect(firstRow).toHaveAttribute("aria-selected", "false");
    await expect(secondRow).toHaveAttribute("aria-selected", "false");
    await expect(defaultRow).toHaveAttribute("aria-selected", "true");
    await secondRow.click();

    await page.locator('[data-testid="sidebar-settings"]:visible').first().click();
    await page.getByTestId("settings-host-section-providers").click();
    await page.getByRole("button", { name: "Oh My Pi provider details", exact: true }).click();
    const settings = page.getByTestId("provider-settings-sheet");
    await expect(settings).toBeVisible();
    await page.getByTestId(`provider-preset-${second}`).getByRole("button").first().click();
    const editor = page.getByTestId("provider-preset-editor");
    await expect(editor).toBeVisible();
    await page.getByTestId("provider-preset-thinking-options").getByRole("button").click();
    await page.getByRole("button", { name: "High", exact: true }).last().click();
    await page.getByTestId("provider-preset-save").click();
    await expect(editor).not.toBeVisible();
    await page.getByLabel("Close", { exact: true }).filter({ visible: true }).last().click();
    await expect(settings).not.toBeVisible();
    await openAgentRoute(page, { workspaceId: workspace.workspaceId, agentId: agent.id });
    await expectComposerVisible(page);

    const logPath = path.join(ompAgentDirectory, "calls.jsonl");
    const before = (await readFile(logPath, "utf8")).trim().split("\n").length;
    await submitMessage(page, "Verify edited preset on the next turn");
    await expect(page.getByText("Hello world", { exact: true })).toBeVisible({ timeout: 30_000 });
    const calls = (await readFile(logPath, "utf8"))
      .trim()
      .split("\n")
      .slice(before)
      .map((line) => JSON.parse(line));
    expect(
      calls.some(
        (call) =>
          (call.type === "set_thinking_level" && call.level === "high") ||
          (Array.isArray(call.args) &&
            call.args.some(
              (arg: string, index: number) =>
                arg === "--thinking" && call.args[index + 1] === "high",
            )),
      ),
    ).toBe(true);
  } finally {
    for (const name of [first, second]) {
      await client.deleteProviderPreset("omp", name, workspace.repoPath).catch(() => undefined);
    }
    await client.close();
    await workspace.cleanup();
  }
});

test("a draft applies saved roles before its first message creates the agent", async ({ page }) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: 1440, height: 1000 });
  const workspace = await seedWorkspace({ repoPrefix: "provider-preset-draft-" });
  const client = await connectDaemonClient<DaemonClient>({ clientIdPrefix: "preset-draft" });
  const model = JSON.stringify(["test", "model"]);
  try {
    const saved = await client.saveProviderPreset(
      "omp",
      {
        name: "DraftReview",
        model,
        thinkingOptionId: "low",
        featureValues: { "modelRole.task": "unset" },
      },
      workspace.repoPath,
    );
    expect(saved.error).toBeNull();
    await gotoWorkspace(page, workspace.workspaceId);
    await clickNewChat(page);
    await expectComposerVisible(page);
    await openProviderModelCard(page, false);
    await page.getByTestId("provider-model-preset-omp-DraftReview").click();
    await submitMessage(page, "Verify the saved model-role configuration");
    await expect
      .poll(
        async () => {
          const entries = (await client.fetchAgents()).entries;
          const agent = entries.find(
            (entry) => entry.agent.workspaceId === workspace.workspaceId,
          )?.agent;
          return agent
            ? {
                provider: agent.provider,
                model: agent.model,
                thinking: agent.thinkingOptionId,
                role: agent.features?.find((feature) => feature.id === "modelRole.task")?.value,
              }
            : null;
        },
        { timeout: 30_000 },
      )
      .toEqual({ provider: "omp", model, thinking: "low", role: "unset" });
  } finally {
    await client
      .deleteProviderPreset("omp", "DraftReview", workspace.repoPath)
      .catch(() => undefined);
    await client.close();
    await workspace.cleanup();
  }
});

for (const mobile of [false, true]) {
  test(`provider settings create a selectable role preset on ${mobile ? "mobile" : "desktop"}`, async ({
    page,
  }, testInfo) => {
    test.setTimeout(120_000);
    await page.setViewportSize(
      mobile ? { width: 390, height: 844 } : { width: 1440, height: 1000 },
    );
    const workspace = await seedWorkspace({ repoPrefix: "provider-model-presets-" });
    const client = await connectDaemonClient<DaemonClient>({ clientIdPrefix: "preset-browser" });
    const presetName = mobile ? "MobileReview" : "DesktopReview";
    try {
      const agent = await client.createAgent({
        provider: "omp",
        cwd: workspace.repoPath,
        workspaceId: workspace.workspaceId,
        model: JSON.stringify(["test", "model"]),
        thinkingOptionId: "high",
        featureValues: { "modelRole.task": "unset", fastMode: true },
        title: "Provider preset browser verification",
      });
      await expect
        .poll(async () => (await client.fetchAgent(agent.id))?.agent.status, { timeout: 30_000 })
        .toBe("idle");
      await openAgentRoute(page, { workspaceId: workspace.workspaceId, agentId: agent.id });
      await expectComposerVisible(page);
      if (mobile) {
        await page.getByRole("button", { name: "Open menu", exact: true }).first().click();
      }
      await page.locator('[data-testid="sidebar-settings"]:visible').first().click();
      await expect(page.getByTestId("settings-sidebar")).toBeVisible();
      await page.getByTestId("settings-host-section-providers").click();
      await expect(page.getByTestId("host-page-providers-card")).toBeVisible();
      await page.getByRole("button", { name: "Oh My Pi provider details", exact: true }).click();
      const settings = page.getByTestId("provider-settings-sheet");
      await expect(settings).toBeVisible();
      await expect(page.getByText("No model presets yet", { exact: true })).toBeVisible();
      await page.getByTestId("provider-presets-empty-create").click();
      const editor = page.getByTestId("provider-preset-editor");
      await expect(editor).toBeVisible();
      await page.getByTestId("provider-preset-name").fill(presetName);
      await page.getByTestId("provider-preset-model-options").getByRole("button").click();
      await page
        .getByRole("button", { name: /other \/ Other/ })
        .last()
        .click();
      await page.getByTestId("provider-preset-thinking-options").getByRole("button").click();
      await page.getByRole("button", { name: "Low", exact: true }).last().click();
      await page.getByTestId("provider-preset-save").click();
      await expect(editor).not.toBeVisible();
      await expect(page.getByTestId(`provider-preset-${presetName}`)).toBeVisible();
      const stored = await client.listProviderPresets("omp", workspace.repoPath);
      expect(stored.presets).toContainEqual(
        expect.objectContaining({ name: presetName, thinkingOptionId: "low", source: "global" }),
      );
      // Reopen the saved form before leaving settings: this verifies native readback through
      // the same editor a user returns to, rather than relying only on the SDK list response.
      await page.getByTestId(`provider-preset-${presetName}`).getByRole("button").first().click();
      await expect(editor).toBeVisible();
      await expect(page.getByTestId("provider-preset-name")).toHaveValue(presetName);
      await expect(page.getByTestId("provider-preset-model-options")).toContainText(
        "other / Other",
      );
      await expect(page.getByTestId("provider-preset-thinking-options")).toContainText("Low");
      await page.getByRole("button", { name: "Cancel", exact: true }).last().click();
      await expect(editor).not.toBeVisible();
      await page.getByLabel("Close", { exact: true }).filter({ visible: true }).last().click();
      await expect(settings).not.toBeVisible();
      // A cold agent route starts fresh model and role discovery together. Presets saved from
      // global settings must appear above Default model and override the selected default model.
      await openAgentRoute(page, { workspaceId: workspace.workspaceId, agentId: agent.id });
      await expectComposerVisible(page);
      await openProviderModelCard(page, mobile);
      const group = page.getByTestId("provider-model-presets-omp");
      await expect(group).toBeVisible();
      const defaultHeading = page.getByText("Default model", { exact: true });
      await expect(defaultHeading).toBeVisible();
      const defaultBox = await defaultHeading.boundingBox();
      const presetBox = await group.boundingBox();
      expect(defaultBox).not.toBeNull();
      expect(presetBox).not.toBeNull();
      expect(presetBox!.y).toBeLessThan(defaultBox!.y);
      await page.screenshot({ path: testInfo.outputPath("model-preset-card.png"), fullPage: true });
      await page.getByTestId(`provider-model-preset-omp-${presetName}`).click();
      await expect
        .poll(async () => {
          const snapshot = (await client.fetchAgent(agent.id))?.agent;
          return {
            model: snapshot?.model,
            thinking: snapshot?.thinkingOptionId,
            role: snapshot?.features?.find((feature) => feature.id === "modelRole.task")?.value,
            fast: snapshot?.features?.find((feature) => feature.id === "fastMode")?.value,
          };
        })
        .toEqual({
          model: JSON.stringify(["other", "model"]),
          thinking: "low",
          role: "native",
          fast: true,
        });
    } finally {
      await client
        .deleteProviderPreset("omp", presetName, workspace.repoPath)
        .catch(() => undefined);
      await client.close();
      await workspace.cleanup();
    }
  });
}

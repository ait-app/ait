import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { test as base, expect, type Page } from "../support/fixtures";
import { repoRoot } from "../support/helpers/ait-daemon";
import { connectDaemonClient } from "../support/helpers/daemon-client-loader";
import { seedWorkspace } from "../support/helpers/seed-client";
import { openAgentRoute } from "../support/helpers/mock-agent";
import { expectComposerVisible, submitMessage } from "../support/helpers/composer";
import { clickNewChat, gotoWorkspace } from "../support/helpers/launcher";

const test = base.extend<{}, { ompAgentDirectory: string }>({
  ompAgentDirectory: [
    // oxlint-disable-next-line no-empty-pattern -- Playwright fixture dependency declaration.
    async ({}, use) => {
      const directory = await mkdtemp(path.join(tmpdir(), "ait-preset-browser-"));
      try {
        await use(directory);
      } finally {
        await rm(directory, { recursive: true, force: true });
      }
    },
    { scope: "worker" },
  ],
  e2eDaemonEnvironment: async ({ ompAgentDirectory }, use) => {
    await use({
      AIT_SERVER_OMP_BIN: path.join(repoRoot, "crates/provider/omp/src/tests/fixtures/omp.py"),
      PI_CODING_AGENT_DIR: ompAgentDirectory,
    });
  },
});

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
      await openProviderModelCard(page, mobile);
      await expect(page.getByTestId("provider-model-presets-omp")).toHaveCount(0);
      await page.getByTestId("selector-header-settings-omp").click();
      const settings = page.getByTestId("provider-settings-sheet");
      await expect(settings).toBeVisible();
      await expect(page.getByText("No model presets yet", { exact: true })).toBeVisible();
      await page.getByTestId("provider-presets-empty-create").click();
      const editor = page.getByTestId("provider-preset-editor");
      await expect(editor).toBeVisible();
      await page.getByTestId("provider-preset-name").fill(presetName);
      await page.getByTestId("provider-preset-model-options").getByRole("button").click();
      await page.getByRole("button", { name: "Fixture model", exact: true }).last().click();
      await page.getByTestId("provider-preset-thinking-options").getByRole("button").click();
      await page.getByRole("button", { name: "Low", exact: true }).last().click();
      await page.getByTestId("provider-preset-save").click();
      await expect(editor).not.toBeVisible();
      await expect(page.getByTestId(`provider-preset-${presetName}`)).toBeVisible();
      const stored = await client.listProviderPresets("omp", workspace.repoPath);
      expect(stored.presets).toContainEqual(
        expect.objectContaining({ name: presetName, thinkingOptionId: "low", source: "global" }),
      );
      await page.getByLabel("Close", { exact: true }).filter({ visible: true }).last().click();
      await expect(settings).not.toBeVisible();
      await openProviderModelCard(page, mobile);
      const group = page.getByTestId("provider-model-presets-omp");
      await expect(group).toBeVisible();
      const defaultHeading = page.getByText("Default model", { exact: true });
      await expect(defaultHeading).toBeVisible();
      const defaultBox = await defaultHeading.boundingBox();
      const presetBox = await group.boundingBox();
      expect(defaultBox).not.toBeNull();
      expect(presetBox).not.toBeNull();
      expect(defaultBox!.y).toBeLessThan(presetBox!.y);
      await group.scrollIntoViewIfNeeded();
      await page.screenshot({ path: testInfo.outputPath("model-preset-card.png"), fullPage: true });
      await page.getByTestId(`provider-model-preset-omp-${presetName}`).click();
      await expect
        .poll(async () => {
          const snapshot = (await client.fetchAgent(agent.id))?.agent;
          return {
            thinking: snapshot?.thinkingOptionId,
            role: snapshot?.features?.find((feature) => feature.id === "modelRole.task")?.value,
            fast: snapshot?.features?.find((feature) => feature.id === "fastMode")?.value,
          };
        })
        .toEqual({ thinking: "low", role: "native", fast: true });
    } finally {
      await client
        .deleteProviderPreset("omp", presetName, workspace.repoPath)
        .catch(() => undefined);
      await client.close();
      await workspace.cleanup();
    }
  });
}

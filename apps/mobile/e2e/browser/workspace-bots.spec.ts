import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { randomUUID } from "node:crypto";
import { expect, test } from "../support/fixtures";
import { gotoWorkspace } from "../support/helpers/launcher";
import { seedWorkspace } from "../support/helpers/seed-client";

test("experimental Bot creation, conversation, settings and persistence", async ({ page }) => {
  test.setTimeout(120_000);
  const workspace = await seedWorkspace({ repoPrefix: "bot-ui-" });
  const client = workspace.client as unknown as DaemonClient;
  try {
    await gotoWorkspace(page, workspace.workspaceId);
    await page.getByTestId("workspace-new-tab-button").filter({ visible: true }).first().click();
    await expect(page.getByTestId("workspace-new-tab-menu-bot")).toHaveCount(0);
    await page.keyboard.press("Escape");
    await page.goto("/settings/about");
    const toggle = page.getByTestId("settings-experimental-features");
    await expect(toggle).toHaveAttribute("aria-checked", "false");
    await toggle.click();
    await expect(toggle).toHaveAttribute("aria-checked", "true");
    await page.reload();
    await expect(toggle).toHaveAttribute("aria-checked", "true");

    await gotoWorkspace(page, workspace.workspaceId);
    await page.getByTestId("workspace-new-tab-button").filter({ visible: true }).first().click();
    await page.getByTestId("workspace-new-tab-menu-bot").filter({ visible: true }).click();
    await page.getByTestId("bot-create-new").click();
    await page.getByTestId("bot-name").fill("Workspace helper");
    await page.getByTestId("bot-instructions").fill("Answer briefly.");
    await page.getByTestId("bot-model-trigger").click();
    await page
      .getByTestId("combobox-desktop-container")
      .getByText("Offline model", { exact: true })
      .click();
    await page.getByTestId("bot-save").click();
    await expect(page.getByTestId("bot-chat")).toBeVisible();
    const { bots } = await client.botRequest("bot.list.request", {
      workspaceId: workspace.workspaceId,
    });
    expect(bots).toHaveLength(1);
    const botId = bots[0].id;
    await page.getByTestId("bot-message-input").fill("Hello Bot");
    await page.getByTestId("bot-message-send").click();
    await expect(page.getByTestId("bot-chat")).toContainText("Echo: Hello Bot", {
      timeout: 30_000,
    });
    await expect(page.getByTestId("bot-message-input")).toHaveValue("");

    await page.getByTestId("bot-show-settings").click();
    await page.getByTestId("bot-name").fill("Renamed helper");
    await page.getByTestId("bot-instructions").fill("Use short answers.");
    await page.getByTestId("bot-save").click();
    await expect
      .poll(async () => (await client.botRequest("bot.get.request", { botId })).bot.name)
      .toBe("Renamed helper");
    await page.getByTestId("bot-show-settings").click();
    await page.getByTestId("bot-memory").fill("Remember this workspace.");
    await page.getByTestId("bot-memory-save").click();
    await expect
      .poll(async () => (await client.botRequest("bot.memory.get.request", { botId })).text)
      .toBe("Remember this workspace.");
    await page.getByTestId("bot-show-chat").click();
    await page.reload();
    await expect(page.getByTestId("bot-chat")).toContainText("Echo: Hello Bot", {
      timeout: 30_000,
    });
    await page.getByTestId("bot-show-settings").click();
    await expect(page.getByTestId("bot-name")).toHaveValue("Renamed helper");
    await expect(page.getByTestId("bot-memory")).toHaveValue("Remember this workspace.");
    await page.screenshot({ path: "/tmp/ait-bot-settings.png" });

    await page.goto("/settings/about");
    await toggle.click();
    await expect(toggle).toHaveAttribute("aria-checked", "false");
    await gotoWorkspace(page, workspace.workspaceId);
    await page.getByTestId("workspace-new-tab-button").filter({ visible: true }).first().click();
    await expect(page.getByTestId("workspace-new-tab-menu-bot")).toHaveCount(0);
    expect((await client.botRequest("bot.get.request", { botId })).bot.name).toBe("Renamed helper");
  } finally {
    await workspace.cleanup();
  }
});

test("Bot answers approvals and protects memory from stale edits", async ({ page }) => {
  const workspace = await seedWorkspace({ repoPrefix: "bot-approval-" });
  const client = workspace.client as unknown as DaemonClient;
  const botId = randomUUID();
  try {
    await client.botRequest("bot.create.request", {
      botId,
      workspaceId: workspace.workspaceId,
      name: "Approval helper",
      instructions: "",
      execution: { provider: "codex", model: "offline-model", modeId: null, thinking: null },
    });
    await page.goto("/settings/about");
    await page.getByTestId("settings-experimental-features").click();
    await gotoWorkspace(page, workspace.workspaceId);
    await page.getByTestId("workspace-new-tab-button").filter({ visible: true }).first().click();
    await page.getByTestId("workspace-new-tab-menu-bot").filter({ visible: true }).click();
    await page.getByTestId(`bot-open-${botId}`).click();
    await page.getByTestId("bot-message-input").fill("permit-question");
    await page.getByTestId("bot-message-send").click();
    const question = page.getByTestId("question-form-card");
    await expect(question).toBeVisible({ timeout: 20_000 });
    await question.getByText("first", { exact: true }).click();
    await question.getByTestId("question-form-primary-action").click();
    await expect(page.getByTestId("bot-chat")).toContainText("Echo: approved", { timeout: 20_000 });
    await expect(question).toHaveCount(0);

    await page.getByTestId("bot-show-settings").click();
    const editor = page.getByTestId("bot-memory");
    await expect(editor).toBeVisible();
    const memory = await client.botRequest("bot.memory.get.request", { botId });
    await editor.fill("Stale local edit");
    await client.botRequest("bot.memory.update.request", {
      botId,
      text: "Changed elsewhere",
      expected: memory.revision,
    });
    await page.getByTestId("bot-memory-save").click();
    await expect(
      page.getByText("The memory changed elsewhere. Reload it before saving again."),
    ).toBeVisible();
    expect((await client.botRequest("bot.memory.get.request", { botId })).text).toBe(
      "Changed elsewhere",
    );
    await page.getByRole("button", { name: "Reload", exact: true }).click();
    await expect(editor).toHaveValue("Changed elsewhere");
    await editor.fill("Current local edit");
    await page.getByTestId("bot-memory-save").click();
    await expect
      .poll(async () => (await client.botRequest("bot.memory.get.request", { botId })).text)
      .toBe("Current local edit");
  } finally {
    await workspace.cleanup();
  }
});

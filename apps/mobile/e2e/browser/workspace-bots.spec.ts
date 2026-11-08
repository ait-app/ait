import type { DaemonClient } from "@ait/client/internal/daemon-client";
import type { Page } from "@playwright/test";
import { expect, test } from "../support/fixtures";
import { gotoWorkspace } from "../support/helpers/launcher";
import { seedWorkspace } from "../support/helpers/seed-client";
import { clickSettingsBackToWorkspace, openSettingsSection } from "../support/helpers/settings";

async function setExperimentalFeatures(page: Page, enabled: boolean): Promise<void> {
  await page.goto("/settings/about");
  const toggle = page.getByTestId("settings-experimental-features");
  await expect(toggle).toHaveAttribute("aria-checked", String(!enabled));
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-checked", String(enabled));
}

async function openNewTabMenu(page: Page): Promise<void> {
  await page.getByTestId("workspace-new-tab-button").filter({ visible: true }).first().click();
}

async function openBotTab(page: Page): Promise<void> {
  await openNewTabMenu(page);
  await page.getByTestId("workspace-new-tab-menu-bot").filter({ visible: true }).click();
  await expect(page.getByTestId("workspace-bot-tab")).toBeVisible();
}

async function submitSetupWithOfflineModel(page: Page): Promise<void> {
  await expect(page.getByTestId("bot-setup")).toBeVisible();
  await page.getByTestId("bot-model-trigger").click();
  await page
    .getByTestId("combobox-desktop-container")
    .getByText("Offline model", { exact: true })
    .click();
  // The permission section shows the defaults and both risk warnings; submit keeps them.
  await expect(page.getByTestId("bot-permissions")).toBeVisible();
  await expect(page.getByTestId("bot-group-observe")).toBeVisible();
  await expect(page.getByTestId("bot-group-agents")).toHaveAttribute("aria-checked", "true");
  await expect(page.getByTestId("bot-group-scripts")).toHaveAttribute("aria-checked", "false");
  await expect(page.getByTestId("bot-mode-cap-warning")).toBeVisible();
  await expect(page.getByTestId("bot-command-reach-warning")).toBeVisible();
  await page.getByTestId("bot-setup-submit").click();
  await expect(page.getByTestId("bot-chat")).toBeVisible();
}

function botInput(page: Page) {
  return page.getByTestId("bot-chat").getByRole("textbox");
}

async function sendBotMessage(page: Page, text: string, reply: string): Promise<void> {
  await botInput(page).fill(text);
  await page.getByTestId("bot-message-send").click();
  await expect(page.getByTestId("bot-chat")).toContainText(reply, { timeout: 30_000 });
  await expect(botInput(page)).toHaveValue("");
}

test("workspace bot setup, conversation, settings and persistence", async ({ page }) => {
  test.setTimeout(180_000);
  const workspace = await seedWorkspace({ repoPrefix: "bot-ui-" });
  const client = workspace.client as unknown as DaemonClient;
  const { workspaceId } = workspace;
  const getBot = async () => (await client.botRequest("bot.get.request", { workspaceId })).bot;
  const getMemory = async () =>
    (await client.botRequest("bot.memory.get.request", { workspaceId })).text;
  try {
    await gotoWorkspace(page, workspaceId);
    await openNewTabMenu(page);
    await expect(page.getByTestId("workspace-new-tab-menu-bot")).toHaveCount(0);
    await page.keyboard.press("Escape");
    await setExperimentalFeatures(page, true);
    await page.reload();
    await expect(page.getByTestId("settings-experimental-features")).toHaveAttribute(
      "aria-checked",
      "true",
    );

    // Opening the tab must not configure anything until the wizard is submitted.
    await gotoWorkspace(page, workspaceId);
    await openBotTab(page);
    await expect(page.getByTestId("bot-setup")).toBeVisible();
    expect(await getBot()).toBeNull();
    await submitSetupWithOfflineModel(page);
    await expect.poll(async () => (await getBot())?.state.kind).toBe("active");
    expect((await getBot())?.execution).toMatchObject({
      provider: "codex",
      model: "offline-model",
    });

    await sendBotMessage(page, "Hello Bot", "Echo: Hello Bot");

    // The prompt and the reply are separate messages, as in a session.
    const chat = page.getByTestId("bot-chat");
    await expect(chat.getByTestId("user-message")).toContainText("Hello Bot");
    await expect(chat.getByTestId("user-message")).not.toContainText("Echo: Hello Bot");
    await expect(chat.getByTestId("assistant-message")).toContainText("Echo: Hello Bot");

    // The hidden session is listed as a subagent and opens read-only.
    const { conversations } = await client.botRequest("bot.conversation.list.request", {
      workspaceId,
    });
    const sessionAgentId = conversations[0]?.sessionAgentId;
    expect(sessionAgentId).toBeTruthy();
    await chat.getByTestId("subagents-track-header").click();
    await page.getByTestId(`subagents-track-row-${sessionAgentId}`).click();
    const session = page.getByTestId("agent-conversation-panel").filter({ visible: true });
    await expect(session).toContainText("Echo: Hello Bot", { timeout: 30_000 });
    await expect(page.getByTestId("message-input-root").filter({ visible: true })).toHaveCount(0);
    await page.getByTestId("workspace-tab-bot").filter({ visible: true }).click();
    await expect(chat).toBeVisible();
    // A finished hidden session must not claim attention and take the tab's focus on return.
    await page.getByTestId("sidebar-settings").click();
    await clickSettingsBackToWorkspace(page);
    await expect(chat).toBeVisible();

    await page.getByTestId("bot-show-settings").click();
    await page.getByTestId("bot-memory").fill("Remember this workspace.");
    await page.getByTestId("bot-memory-save").click();
    await expect.poll(getMemory).toBe("Remember this workspace.");
    await page.getByTestId("bot-show-chat").click();

    // The restored tab is the same singleton Bot with its conversation intact.
    await page.reload();
    await expect(page.getByTestId("bot-chat")).toContainText("Echo: Hello Bot", {
      timeout: 30_000,
    });
    await expect(page.getByTestId("workspace-tab-bot").filter({ visible: true })).toHaveCount(1);
    // The singleton tab is already open, so the launcher no longer offers another Bot.
    await openNewTabMenu(page);
    await expect(page.getByTestId("workspace-new-tab-menu-bot")).toHaveCount(0);
    await page.keyboard.press("Escape");
    await page.getByTestId("bot-show-settings").click();
    await expect(page.getByTestId("bot-memory")).toHaveValue("Remember this workspace.");

    await page.getByTestId("bot-toggle-state").click();
    await expect.poll(async () => (await getBot())?.state.kind).toBe("paused");
    await page.getByTestId("bot-show-chat").click();
    await expect(page.getByTestId("bot-chat")).toContainText("Paused");
    await expect(botInput(page)).not.toBeEditable();
    await expect(page.getByTestId("bot-message-send")).toHaveCount(0);
    // A fresh load reads the paused Bot's history without reopening its conversation.
    await page.reload();
    await expect(page.getByTestId("bot-chat")).toContainText("Echo: Hello Bot", {
      timeout: 30_000,
    });
    await expect(page.getByTestId("bot-chat")).toContainText("Paused");
    await expect(botInput(page)).not.toBeEditable();
    await expect(page.getByTestId("bot-message-send")).toHaveCount(0);
    await page.getByTestId("bot-resume").click();
    await expect.poll(async () => (await getBot())?.state.kind).toBe("active");
    await expect(botInput(page)).toBeEditable();
    await sendBotMessage(page, "After resume", "Echo: After resume");

    // Removing returns to the wizard; setting up again keeps memory but starts a new history.
    await page.getByTestId("bot-show-settings").click();
    page.once("dialog", (dialog) => void dialog.accept());
    await page.getByTestId("bot-remove").click();
    await expect(page.getByTestId("bot-setup")).toBeVisible();
    expect(await getBot()).toBeNull();
    // The retired session's tab goes with its Bot.
    await expect(page.getByTestId(`workspace-tab-agent_${sessionAgentId}`)).toHaveCount(0);
    await submitSetupWithOfflineModel(page);
    await expect(page.getByTestId("bot-chat")).toContainText(
      "Send a message to start a conversation.",
    );
    await expect(page.getByTestId("bot-chat")).not.toContainText("Echo: Hello Bot");
    await page.getByTestId("bot-show-settings").click();
    await expect(page.getByTestId("bot-memory")).toHaveValue("Remember this workspace.");
    expect(await getMemory()).toBe("Remember this workspace.");

    // Disabling the experiment disables the open tab without touching the server Bot. Toggle
    // in-app: a cold load of a non-workspace route currently drops saved tab layouts.
    await page.getByTestId("sidebar-settings").click();
    await openSettingsSection(page, "about");
    const toggle = page.getByTestId("settings-experimental-features");
    await expect(toggle).toHaveAttribute("aria-checked", "true");
    await toggle.click();
    await expect(toggle).toHaveAttribute("aria-checked", "false");
    await clickSettingsBackToWorkspace(page);
    await expect(page.getByTestId("workspace-tab-bot").filter({ visible: true })).toHaveCount(1);
    await expect(
      page.getByText("Enable Experimental features in Settings → About to use Bots."),
    ).toBeVisible();
    await openNewTabMenu(page);
    await expect(page.getByTestId("workspace-new-tab-menu-bot")).toHaveCount(0);
    expect((await getBot())?.state.kind).toBe("active");
  } finally {
    await workspace.cleanup();
  }
});

test("bot denies approvals automatically and protects memory from stale edits", async ({
  page,
}) => {
  test.setTimeout(120_000);
  const workspace = await seedWorkspace({ repoPrefix: "bot-approval-" });
  const client = workspace.client as unknown as DaemonClient;
  const { workspaceId } = workspace;
  try {
    await client.botRequest("bot.setup.request", {
      workspaceId,
      execution: { provider: "codex", model: "offline-model", modeId: null, thinking: null },
    });
    await setExperimentalFeatures(page, true);
    await gotoWorkspace(page, workspaceId);
    await openBotTab(page);
    await expect(page.getByTestId("bot-chat")).toBeVisible();
    await botInput(page).fill("permit-question");
    await page.getByTestId("bot-message-send").click();
    await expect(page.getByTestId("bot-chat")).toContainText("Echo: denied", { timeout: 30_000 });
    await expect(page.getByTestId("question-form-card")).toHaveCount(0);
    await sendBotMessage(page, "continue after denial", "Echo: continue after denial");

    await page.getByTestId("bot-show-settings").click();
    const editor = page.getByTestId("bot-memory");
    await expect(editor).toBeVisible();
    const memory = await client.botRequest("bot.memory.get.request", { workspaceId });
    await editor.fill("Stale local edit");
    await client.botRequest("bot.memory.update.request", {
      workspaceId,
      text: "Changed elsewhere",
      expected: memory.revision,
    });
    await page.getByTestId("bot-memory-save").click();
    await expect(
      page.getByText("The memory changed elsewhere. Reload it before saving again."),
    ).toBeVisible();
    expect((await client.botRequest("bot.memory.get.request", { workspaceId })).text).toBe(
      "Changed elsewhere",
    );
    await page.getByRole("button", { name: "Reload", exact: true }).click();
    await expect(editor).toHaveValue("Changed elsewhere");
    await editor.fill("Current local edit");
    await page.getByTestId("bot-memory-save").click();
    await expect
      .poll(async () => (await client.botRequest("bot.memory.get.request", { workspaceId })).text)
      .toBe("Current local edit");
  } finally {
    await workspace.cleanup();
  }
});

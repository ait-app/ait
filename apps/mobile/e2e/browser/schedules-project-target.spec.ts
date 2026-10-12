import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { expect, test, type Page } from "../support/fixtures";
import { gotoWorkspace } from "../support/helpers/launcher";
import { seedWorkspace, type SeededWorkspace } from "../support/helpers/seed-client";
import { getServerId } from "../support/helpers/server-id";
import { buildSchedulesRoute } from "../../src/utils/host-routes";

function schedules(workspace: SeededWorkspace): DaemonClient {
  return workspace.client as unknown as DaemonClient;
}

async function openScheduleTab(page: Page) {
  await page.getByTestId("workspace-new-tab-button").filter({ visible: true }).first().click();
  const menu = page.getByTestId("workspace-new-tab-menu-schedule").filter({ visible: true });
  await expect(menu).toBeVisible();
  await menu.click();
  await expect(page.getByTestId("workspace-schedule-form")).toBeVisible();
}

async function selectExistingAgent(page: Page, agentId: string) {
  await page.getByTestId("schedule-target-kind-trigger").click();
  await page.getByTestId("schedule-target-agent").click();
  await page.getByTestId("schedule-agent-trigger").click();
  await page.getByTestId(`schedule-agent-option-${agentId}`).click();
  await expect(page.getByTestId(`schedule-agent-option-${agentId}`)).not.toBeVisible();
}

async function selectModel(page: Page) {
  await page.getByTestId("schedule-model-trigger").click();
  const popup = page.getByTestId("combobox-desktop-container");
  await expect(popup).toBeVisible();
  await page.getByTestId("model-search-input").first().fill("Offline model");
  await popup.getByText("Offline model", { exact: true }).click();
}

test.describe("Workspace scheduled tasks", () => {
  const cleanupTasks: (() => Promise<void>)[] = [];
  test.afterEach(async ({ page }, testInfo) => {
    if (testInfo.error) console.error(testInfo.error.message, "at", page.url());
    for (const cleanup of cleanupTasks.toReversed()) await cleanup();
    cleanupTasks.length = 0;
  });

  test("existing-agent task stays in its workspace through edit, run, reload and history", async ({
    page,
  }) => {
    const workspace = await seedWorkspace({
      repoPrefix: "schedule-owned-",
      title: "Scheduled workspace",
    });
    const foreign = await seedWorkspace({ repoPrefix: "schedule-foreign-" });
    cleanupTasks.push(workspace.cleanup, foreign.cleanup);
    const agent = await workspace.client.createAgent({
      provider: "codex",
      cwd: workspace.repoPath,
      workspaceId: workspace.workspaceId,
      title: "Scheduled agent",
      model: "offline-model",
    });
    const foreignAgent = await foreign.client.createAgent({
      provider: "codex",
      cwd: foreign.repoPath,
      workspaceId: foreign.workspaceId,
      title: "Foreign agent",
      model: "offline-model",
    });
    await gotoWorkspace(page, workspace.workspaceId);
    await openScheduleTab(page);
    await expect(page.getByTestId("schedule-workspace-target")).toContainText(
      workspace.workspaceName,
    );
    await page.getByTestId("schedule-target-kind-trigger").click();
    await page.getByTestId("schedule-target-agent").click();
    await page.getByTestId("schedule-agent-trigger").click();
    await expect(page.getByTestId(`schedule-agent-option-${foreignAgent.id}`)).toHaveCount(0);
    await page.getByTestId(`schedule-agent-option-${agent.id}`).click();
    await page.getByTestId("schedule-name-input").fill("Workspace review");
    await page.getByTestId("schedule-prompt-input").fill("Review the current workspace.");
    await page.getByTestId("schedule-max-runs-input").fill("1.5");
    await expect(page.getByTestId("schedule-form-submit")).toBeDisabled();
    await page.getByTestId("schedule-max-runs-input").fill("2");
    await page.getByTestId("schedule-form-submit").click();
    const detail = page.getByTestId("workspace-schedule-details");
    await expect(detail).toContainText("Workspace review");
    const list = await schedules(workspace).scheduleList({ workspaceId: workspace.workspaceId });
    const schedule = list.schedules.find((item) => item.name === "Workspace review");
    expect(schedule).toMatchObject({
      workspaceId: workspace.workspaceId,
      target: { type: "agent", agentId: agent.id },
      cadence: { type: "every", everyMs: 3_600_000 },
      maxRuns: 2,
      status: "active",
    });
    if (!schedule) throw new Error("Created task is missing");
    cleanupTasks.push(async () => {
      await schedules(workspace).scheduleDelete({ id: schedule.id });
    });
    await page.getByTestId("schedule-toggle-status").click();
    await expect(detail).toContainText("Paused");
    await page.getByTestId("schedule-edit").click();
    await expect(page.getByTestId("schedule-prompt-input")).toHaveValue(
      "Review the current workspace.",
    );
    await page.getByTestId("schedule-prompt-input").fill("Review the workspace again.");
    await page.getByTestId("schedule-form-submit").click();
    await expect(detail).toContainText("Review the workspace again.");
    await page.getByTestId("schedule-run-now").click();
    const runCard = detail
      .locator('[data-testid^="schedule-run-"]')
      .filter({ has: page.getByText("Succeeded", { exact: true }) });
    await expect(runCard).toBeVisible({ timeout: 30_000 });
    const inspected = await schedules(workspace).scheduleInspect({ id: schedule.id });
    expect(inspected.schedule?.runs).toHaveLength(1);
    expect(inspected.schedule?.runs[0]).toMatchObject({
      agentId: agent.id,
      workspaceId: workspace.workspaceId,
      status: "succeeded",
    });
    await runCard.getByRole("button").first().click();
    await expect(detail.locator('[data-testid^="schedule-run-output-"]')).not.toBeEmpty();
    await page.reload();
    await expect(detail).toContainText("Workspace review", { timeout: 30_000 });
    await expect(detail).toContainText("Succeeded");
    await page.getByTestId("workspace-schedules-browse").click();
    await expect(page.getByTestId(`workspace-schedule-${schedule.id}`)).toBeVisible();
    await page.reload();
    await expect(page.getByTestId(`workspace-schedule-${schedule.id}`)).toBeVisible({
      timeout: 30_000,
    });
    await page.getByTestId(`workspace-schedule-${schedule.id}`).click();
    await page.getByTestId("schedule-run-now").click();
    await expect(detail).toContainText("Finished", { timeout: 30_000 });
    await expect(page.getByTestId("schedule-run-now")).toHaveCount(0);
    await page.screenshot({ path: "/tmp/workspace-schedule-desktop.png", fullPage: true });
    page.once("dialog", (dialog) => {
      void dialog.accept();
    });
    await page.getByTestId("schedule-delete").click();
    await expect(page.getByTestId("workspace-schedules-list")).toBeVisible();
    await expect(page.getByTestId(`workspace-schedule-${schedule.id}`)).toHaveCount(0);
  });

  test("new-agent task creates in the owning workspace and preserves its workspace on completion", async ({
    page,
  }) => {
    const workspace = await seedWorkspace({
      repoPrefix: "schedule-new-agent-",
      title: "New agent owner",
    });
    cleanupTasks.push(workspace.cleanup);
    await gotoWorkspace(page, workspace.workspaceId);
    await openScheduleTab(page);
    await selectModel(page);
    await page.getByTestId("schedule-name-input").fill("New agent review");
    await page.getByTestId("schedule-prompt-input").fill("Say hello from a new scheduled agent.");
    await page.getByTestId("schedule-cadence-kind-trigger").click();
    await page.getByTestId("schedule-cadence-kind-once").click();
    await expect(page.getByTestId("cadence-once-at")).toBeVisible();
    await expect(page.getByTestId("schedule-start-at-input")).toHaveCount(0);
    await page.getByTestId("schedule-form-submit").click();
    const detail = page.getByTestId("workspace-schedule-details");
    await expect(detail).toContainText("New agent review");
    const list = await schedules(workspace).scheduleList({ workspaceId: workspace.workspaceId });
    const schedule = list.schedules.find((item) => item.name === "New agent review");
    expect(schedule).toMatchObject({
      workspaceId: workspace.workspaceId,
      cadence: { type: "once" },
      target: { type: "new-agent", config: { provider: "codex", archiveOnFinish: true } },
    });
    if (!schedule) throw new Error("Created task is missing");
    cleanupTasks.push(async () => {
      await schedules(workspace).scheduleDelete({ id: schedule.id });
    });
    await page.getByTestId("schedule-run-now").click();
    await expect(detail).toContainText("Succeeded", { timeout: 30_000 });
    await expect(detail).toContainText("Finished");
    const inspected = await schedules(workspace).scheduleInspect({ id: schedule.id });
    expect(inspected.schedule?.runs[0]).toMatchObject({
      workspaceId: workspace.workspaceId,
      status: "succeeded",
    });
    const workspaces = await workspace.client.fetchWorkspaces();
    expect(workspaces.entries.map((item) => item.id)).toContain(workspace.workspaceId);
    await page.reload();
    await expect(detail).toContainText("Finished", { timeout: 30_000 });
  });

  test("global creation selects a workspace on narrow screens and dismisses cleanly", async ({
    page,
  }) => {
    const workspace = await seedWorkspace({
      repoPrefix: "schedule-mobile-",
      title: "Mobile scheduled workspace",
    });
    cleanupTasks.push(workspace.cleanup);
    const agent = await workspace.client.createAgent({
      provider: "codex",
      cwd: workspace.repoPath,
      workspaceId: workspace.workspaceId,
      title: "Mobile scheduled agent",
      model: "offline-model",
    });
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(buildSchedulesRoute());
    await page.getByTestId("schedules-empty-new").click();
    await expect(page.getByTestId("schedule-workspace-sheet")).toBeVisible();
    await page.getByTestId("schedule-workspace-trigger").click();
    await page
      .getByTestId(`schedule-workspace-option-${getServerId()}-${workspace.workspaceId}`)
      .click();
    await expect(page.getByTestId("schedule-form-sheet")).toBeVisible();
    await selectExistingAgent(page, agent.id);
    await page.getByTestId("schedule-name-input").fill("Mobile review");
    await page.getByTestId("schedule-prompt-input").fill("Review on mobile.");
    await page.screenshot({ path: "/tmp/workspace-schedule-mobile.png", fullPage: true });
    await page.getByTestId("schedule-form-submit").click();
    await expect(page.getByTestId("schedule-form-sheet")).toHaveCount(0);
    const list = await schedules(workspace).scheduleList({ workspaceId: workspace.workspaceId });
    const schedule = list.schedules.find((item) => item.name === "Mobile review");
    expect(schedule?.workspaceId).toBe(workspace.workspaceId);
    if (!schedule) throw new Error("Created task is missing");
    cleanupTasks.push(async () => {
      await schedules(workspace).scheduleDelete({ id: schedule.id });
    });
    await expect(page.getByTestId(`schedule-row-${schedule.id}`)).toBeVisible();
    await page.getByTestId("schedules-new").click();
    await expect(page.getByTestId("schedule-workspace-sheet")).toBeVisible();
    await page.getByRole("button", { name: "Close", exact: true }).last().click();
    await expect(page.getByTestId("schedule-workspace-sheet")).toHaveCount(0);
  });
});

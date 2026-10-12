import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { expect, test, type Page } from "../support/fixtures";
import { gotoWorkspace } from "../support/helpers/launcher";
import { seedWorkspace, type SeededWorkspace } from "../support/helpers/seed-client";
import { getServerId } from "../support/helpers/server-id";
import { buildSchedulesRoute } from "../../src/utils/host-routes";
import {
  addScheduleHostAndReload,
  createScheduleHost,
  SECONDARY_MODEL_LABEL,
} from "../support/helpers/schedule-host";

function schedules(workspace: SeededWorkspace): DaemonClient {
  return workspace.client as unknown as DaemonClient;
}

async function openScheduleTab(page: Page) {
  await page.getByTestId("workspace-new-tab-button").filter({ visible: true }).first().click();
  const menu = page.getByTestId("workspace-new-tab-menu-schedule").filter({ visible: true });
  await expect(menu).toBeVisible();
  await menu.click();
  await expect(page.getByTestId("workspace-schedules-list")).toBeVisible();
}

async function selectExistingAgent(page: Page, agentId: string) {
  await page.getByTestId("schedule-execution-agent-trigger").click();
  await page.getByTestId(`schedule-agent-option-${agentId}`).click();
  await expect(page.getByTestId(`schedule-agent-option-${agentId}`)).not.toBeVisible();
}

async function selectModel(page: Page, label = "Offline model") {
  await page.getByTestId("schedule-model-trigger").click();
  const popup = page.getByTestId("combobox-desktop-container");
  await expect(popup).toBeVisible();
  await page.getByTestId("model-search-input").first().fill(label);
  await popup.getByText(label, { exact: true }).click();
}

async function selectPlacement(page: Page, workspace: SeededWorkspace, serverId = getServerId()) {
  await page.getByTestId("schedule-host-trigger").click();
  await page.getByTestId(`schedule-host-option-${serverId}`).click();
  await page.getByTestId("schedule-project-trigger").click();
  await page.getByTestId(`schedule-project-option-${serverId}-${workspace.projectId}`).click();
  await page.getByTestId("schedule-workspace-trigger").click();
  await page.getByTestId(`schedule-workspace-option-${serverId}-${workspace.workspaceId}`).click();
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
    await page.getByTestId("workspace-schedules-new").click();
    await expect(page.getByTestId("schedule-max-runs-input")).toHaveValue("1");
    await expect(page.getByTestId("schedule-workspace-target")).toContainText(
      workspace.workspaceName,
    );
    await selectModel(page);
    await expect(page.getByTestId("schedule-target-kind-trigger")).toHaveCount(0);
    await expect(page.getByTestId("schedule-agent-trigger")).toHaveCount(0);
    await page.getByTestId("schedule-execution-agent-trigger").click();
    await expect(page.getByTestId("schedule-target-new-agent")).toBeVisible();
    await expect(page.getByTestId(`schedule-agent-option-${foreignAgent.id}`)).toHaveCount(0);
    await page.screenshot({
      path: "/tmp/schedule-execution-agent-picker-desktop.png",
      fullPage: true,
    });
    await page.getByPlaceholder("Search agents...").fill("Scheduled agent");
    await expect(page.getByTestId(`schedule-agent-option-${agent.id}`)).toBeVisible();
    await page.getByTestId(`schedule-agent-option-${agent.id}`).click();
    await expect(page.getByTestId("schedule-execution-agent-trigger")).toContainText(
      "Scheduled agent",
    );
    await expect(page.getByTestId("schedule-model-trigger")).toHaveCount(0);
    await page.getByTestId("schedule-name-input").fill("Workspace review");
    await page.getByTestId("schedule-prompt-input").fill("Review the current workspace.");
    await page.screenshot({
      path: "/tmp/schedule-existing-agent-form-desktop.png",
      fullPage: true,
    });
    await page.getByTestId("schedule-execution-agent-trigger").click();
    await page.getByTestId("schedule-target-new-agent").click();
    await expect(page.getByTestId("schedule-model-trigger")).toContainText("Offline model");
    await expect(page.getByTestId("schedule-prompt-input")).toHaveValue(
      "Review the current workspace.",
    );
    await selectExistingAgent(page, agent.id);
    await page.getByTestId("schedule-max-runs-input").fill("1.5");
    await expect(page.getByTestId("schedule-form-submit")).toBeDisabled();
    await page.getByTestId("schedule-max-runs-input").fill("2");
    await page.getByTestId("schedule-form-submit").click();
    const detail = page.getByTestId("workspace-schedule-details");
    await expect(page.getByTestId("workspace-schedules-list")).toContainText("Workspace review");
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
    await page.getByTestId(`workspace-schedule-history-${schedule.id}`).click();
    await page.getByTestId("schedule-toggle-status").click();
    await expect(detail).toContainText("Paused");
    await page.getByTestId("schedule-edit").click();
    await expect(page.getByTestId("schedule-execution-agent-trigger")).toContainText(
      "Scheduled agent",
    );
    await expect(page.getByTestId("schedule-prompt-input")).toHaveValue(
      "Review the current workspace.",
    );
    await page.getByTestId("schedule-prompt-input").fill("Review the workspace again.");
    await page.getByTestId("schedule-form-submit").click();
    await expect(page.getByTestId(`workspace-schedule-${schedule.id}`)).toContainText(
      "Review the workspace again.",
    );
    await page.getByTestId(`workspace-schedule-history-${schedule.id}`).click();
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
    await expect(page.getByTestId("workspace-schedules-filter-paused")).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await expect(page.getByTestId(`workspace-schedule-${schedule.id}`)).toBeVisible();
    await page.reload();
    await expect(page.getByTestId(`workspace-schedule-${schedule.id}`)).toBeVisible({
      timeout: 30_000,
    });
    await page.getByTestId(`workspace-schedule-history-${schedule.id}`).click();
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
    await page.getByTestId("workspace-schedules-new").click();
    await expect(page.getByTestId("schedule-max-runs-input")).toHaveValue("1");
    await selectModel(page);
    await page.getByTestId("schedule-name-input").fill("New agent review");
    await page.getByTestId("schedule-prompt-input").fill("Say hello from a new scheduled agent.");
    await page.getByTestId("schedule-cadence-kind-trigger").click();
    await expect(page.getByTestId("schedule-cadence-kind-once")).toHaveCount(0);
    await page.getByTestId("schedule-cadence-kind-every").click();
    await expect(page.getByTestId("schedule-start-at-input")).toBeVisible();
    await page.getByTestId("schedule-form-submit").click();
    const detail = page.getByTestId("workspace-schedule-details");
    await expect(page.getByTestId("workspace-schedules-list")).toContainText("New agent review");
    const list = await schedules(workspace).scheduleList({ workspaceId: workspace.workspaceId });
    const schedule = list.schedules.find((item) => item.name === "New agent review");
    expect(schedule).toMatchObject({
      workspaceId: workspace.workspaceId,
      cadence: { type: "every", everyMs: 3_600_000 },
      maxRuns: 1,
      target: { type: "new-agent", config: { provider: "codex", archiveOnFinish: true } },
    });
    if (!schedule) throw new Error("Created task is missing");
    cleanupTasks.push(async () => {
      await schedules(workspace).scheduleDelete({ id: schedule.id });
    });
    await page.getByTestId(`workspace-schedule-history-${schedule.id}`).click();
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

  test("global creation selects host, project and workspace on narrow screens and dismisses cleanly", async ({
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
    await expect(page.getByTestId("schedule-project-trigger")).toBeDisabled();
    await expect(page.getByTestId("schedule-workspace-trigger")).toBeDisabled();
    await expect(page.getByTestId("schedule-execution-agent-trigger")).toHaveCount(0);
    await expect(page.getByTestId("schedule-host-trigger")).toBeInViewport({ ratio: 1 });
    await page.screenshot({ path: "/tmp/schedule-global-placement-mobile.png", fullPage: true });
    await selectPlacement(page, workspace);
    await expect(page.getByTestId("schedule-form-sheet")).toBeVisible();
    await page.getByTestId("schedule-execution-agent-trigger").click();
    await expect(page.getByPlaceholder("Search agents...")).toBeVisible();
    await expect(page.getByTestId(`schedule-agent-option-${agent.id}`)).toBeInViewport();
    await page.screenshot({
      path: "/tmp/schedule-execution-agent-picker-mobile.png",
      fullPage: true,
    });
    await page.getByTestId(`schedule-agent-option-${agent.id}`).click();
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
    await expect(page.getByTestId("schedule-host-trigger")).toContainText("Select host");
    await page.getByRole("button", { name: "Close", exact: true }).last().click();
    await expect(page.getByTestId("schedule-workspace-sheet")).toHaveCount(0);
  });

  test("global creation resets its placement and targets the selected host's worktree", async ({
    page,
  }) => {
    test.setTimeout(120_000);
    const local = await seedWorkspace({
      repoPrefix: "schedule-global-local-",
      title: "Local workspace",
    });
    const otherProject = await seedWorkspace({
      repoPrefix: "schedule-global-other-",
      title: "Other project workspace",
    });
    cleanupTasks.push(local.cleanup, otherProject.cleanup);
    const remote = await createScheduleHost();
    cleanupTasks.push(remote.cleanup);
    const created = await remote.client.createWorkspace({
      source: {
        kind: "worktree",
        projectId: remote.projectId,
        baseBranch: "main",
        worktreeSlug: "scheduled-worktree",
      },
      title: "Scheduled worktree",
    });
    if (!created.workspace) throw new Error(created.error ?? "Missing worktree");
    const worktree = created.workspace;
    const agent = await local.client.createAgent({
      provider: "codex",
      cwd: local.workspaceDirectory,
      workspaceId: local.workspaceId,
      title: "Local existing agent",
      model: "offline-model",
    });
    await page.goto(buildSchedulesRoute());
    await addScheduleHostAndReload({
      page,
      serverId: remote.serverId,
      port: remote.port,
      label: remote.label,
    });
    await page.getByTestId("schedules-empty-new").click();
    await selectPlacement(page, local);
    await selectExistingAgent(page, agent.id);
    await page.getByTestId("schedule-prompt-input").fill("Local draft");

    await page.getByTestId("schedule-project-trigger").click();
    await expect(
      page.getByTestId(`schedule-project-option-${remote.serverId}-${remote.projectId}`),
    ).toHaveCount(0);
    await page
      .getByTestId(`schedule-project-option-${getServerId()}-${otherProject.projectId}`)
      .click();
    await expect(page.getByTestId("schedule-workspace-trigger")).toContainText(
      "Select a workspace",
    );
    await expect(page.getByTestId("schedule-execution-agent-trigger")).toHaveCount(0);
    await page.getByTestId("schedule-workspace-trigger").click();
    await expect(
      page.getByTestId(`schedule-workspace-option-${getServerId()}-${local.workspaceId}`),
    ).toHaveCount(0);
    await page
      .getByTestId(`schedule-workspace-option-${getServerId()}-${otherProject.workspaceId}`)
      .click();
    await expect(page.getByTestId("schedule-prompt-input")).toHaveValue("");

    await page.getByTestId("schedule-host-trigger").click();
    await page.getByTestId(`schedule-host-option-${remote.serverId}`).click();
    await expect(page.getByTestId("schedule-project-trigger")).toContainText("Select project");
    await expect(page.getByTestId("schedule-workspace-trigger")).toBeDisabled();
    await expect(page.getByTestId("schedule-execution-agent-trigger")).toHaveCount(0);
    await page.getByTestId("schedule-project-trigger").click();
    await expect(
      page.getByTestId(`schedule-project-option-${getServerId()}-${local.projectId}`),
    ).toHaveCount(0);
    await page
      .getByTestId(`schedule-project-option-${remote.serverId}-${remote.projectId}`)
      .click();
    await page.getByTestId("schedule-workspace-trigger").click();
    await page.getByTestId(`schedule-workspace-option-${remote.serverId}-${worktree.id}`).click();
    await expect(page.getByTestId("schedule-workspace-trigger")).toContainText(
      "Scheduled worktree",
    );
    await expect(page.getByTestId("schedule-max-runs-input")).toHaveValue("1");
    await page.getByTestId("schedule-execution-agent-trigger").click();
    await expect(page.getByTestId(`schedule-agent-option-${agent.id}`)).toHaveCount(0);
    await page.getByTestId("schedule-target-new-agent").click();
    await selectModel(page, SECONDARY_MODEL_LABEL);
    await page.getByTestId("schedule-name-input").fill("Global worktree task");
    await page.getByTestId("schedule-prompt-input").fill("Review the selected worktree.");
    await page.getByTestId("schedule-host-trigger").scrollIntoViewIfNeeded();
    await page.screenshot({ path: "/tmp/schedule-global-placement-desktop.png", fullPage: true });
    await page.getByTestId("schedule-form-submit").click();
    await expect(page.getByTestId("schedule-form-sheet")).toHaveCount(0);
    const list = await schedules(remote).scheduleList({ workspaceId: worktree.id });
    const schedule = list.schedules.find((item) => item.name === "Global worktree task");
    expect(schedule).toMatchObject({
      workspaceId: worktree.id,
      maxRuns: 1,
      target: {
        type: "new-agent",
        config: { cwd: worktree.workspaceDirectory, model: "secondary-offline-model" },
      },
    });
    if (!schedule) throw new Error("Created worktree task is missing");
    cleanupTasks.push(async () => {
      await schedules(remote).scheduleDelete({ id: schedule.id });
    });
    const localList = await schedules(local).scheduleList({ workspaceId: local.workspaceId });
    expect(localList.schedules.some((item) => item.name === "Global worktree task")).toBe(false);
    await expect(page.getByTestId(`schedule-row-${schedule.id}`)).toBeVisible();
  });
  test("opens an upcoming timeline ordered by trigger time and edits directly from it", async ({
    page,
  }) => {
    const workspace = await seedWorkspace({
      repoPrefix: "schedule-timeline-",
      title: "Future tasks",
    });
    cleanupTasks.push(workspace.cleanup);
    const agent = await workspace.client.createAgent({
      provider: "codex",
      cwd: workspace.repoPath,
      workspaceId: workspace.workspaceId,
      model: "offline-model",
    });
    const create = async (name: string, hours: number) => {
      const result = await schedules(workspace).scheduleCreate({
        workspaceId: workspace.workspaceId,
        name,
        prompt: name,
        cadence: { type: "every", everyMs: 3_600_000 },
        startAt: new Date(Date.now() + hours * 3_600_000).toISOString(),
        target: { type: "agent", agentId: agent.id },
        maxRuns: 1,
        runOnCreate: false,
      });
      if (!result.schedule) throw new Error(result.error ?? "Missing task");
      const id = result.schedule.id;
      cleanupTasks.push(async () => {
        await schedules(workspace).scheduleDelete({ id });
      });
      return id;
    };
    const earlier = await create("Earlier review", 2);
    const later = await create("Later review", 6);
    await gotoWorkspace(page, workspace.workspaceId);
    await openScheduleTab(page);
    await expect(page.getByTestId("workspace-schedule-form")).toHaveCount(0);
    const rows = page
      .getByTestId("workspace-schedules-list")
      .locator(
        '[data-testid^="workspace-schedule-"]:not([data-testid^="workspace-schedule-history-"])',
      );
    await expect(rows).toHaveCount(2);
    await expect(rows.nth(0)).toHaveAttribute("data-testid", `workspace-schedule-${earlier}`);
    await expect(rows.nth(1)).toHaveAttribute("data-testid", `workspace-schedule-${later}`);
    await expect(page.getByTestId(`schedule-next-trigger-${earlier}`)).not.toBeEmpty();
    await page.screenshot({ path: "/tmp/schedule-timeline-desktop.png", fullPage: true });
    await page.getByTestId(`workspace-schedule-${earlier}`).click();
    await expect(page.getByTestId("schedule-prompt-input")).toHaveValue("Earlier review");
    await expect(page.getByTestId("schedule-max-runs-input")).toHaveValue("1");
    await page.getByTestId("schedule-name-input").fill("Edited earlier review");
    await page.getByTestId("schedule-form-submit").click();
    await expect(page.getByTestId(`workspace-schedule-${earlier}`)).toContainText(
      "Edited earlier review",
    );
    await page.reload();
    await expect(page.getByTestId(`workspace-schedule-${earlier}`)).toBeVisible({
      timeout: 30_000,
    });
    await page.setViewportSize({ width: 390, height: 844 });
    await expect(page.getByTestId(`workspace-schedule-${earlier}`)).toBeVisible();
    await page.screenshot({ path: "/tmp/schedule-timeline-mobile.png", fullPage: true });
  });
});

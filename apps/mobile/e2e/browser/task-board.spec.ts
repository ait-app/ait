import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { expect, test } from "../support/fixtures";
import { seedWorkspace, type SeededWorkspace } from "../support/helpers/seed-client";
import { getServerId } from "../support/helpers/server-id";

test.describe("Task overview", () => {
  let workspace: SeededWorkspace;
  const errors: string[] = [];
  test.beforeEach(async ({ page }) => {
    errors.length = 0;
    page.on("pageerror", (error) => errors.push(error.message));
    await page.addInitScript(() =>
      localStorage.setItem(
        "@paseo:app-settings",
        JSON.stringify({ theme: "light", language: "zh-CN" }),
      ),
    );
    workspace = await seedWorkspace({ repoPrefix: "task-board-", title: "Ait" });
    await workspace.client.renameProject(workspace.projectId, "Ait");
  });
  test.afterEach(async () => {
    await workspace?.cleanup();
  });

  test("groups live tasks, filters, responds to permissions, stops, reviews and archives", async ({
    page,
  }, testInfo) => {
    await page.setViewportSize({ width: 1717, height: 916 });
    const client = workspace.client as unknown as DaemonClient;
    const specs: [string, string | null][] = [
      ["修复登录回调", "permit-command"],
      ["修复 CI 构建", "fail"],
      ["优化工作区加载", "stream"],
      ["补充中继连接测试", "stream"],
      ["整理发布说明", "stream"],
      ["新增定时任务模板", "模板已实现，并补充了回归测试。"],
      ["更新快捷键提示", "已更新快捷键提示。"],
      ["修复移动端滚动", null],
      ["更新文档链接", null],
    ];
    const ids: string[] = [];
    for (const [title, prompt] of specs) {
      const agent = await client.createAgent({
        provider: "codex",
        cwd: workspace.repoPath,
        workspaceId: workspace.workspaceId,
        title,
        model: "offline-model",
      });
      ids.push(agent.id);
      if (title === "修复登录回调") {
        await client.sendAgentMessage(agent.id, "回调参数校验已修复，等待运行测试后确认结果。");
        await workspace.client.waitForFinish(agent.id);
      }
      if (prompt) await client.sendAgentMessage(agent.id, prompt);
    }
    await client.createAgent({
      provider: "codex",
      cwd: workspace.repoPath,
      workspaceId: workspace.workspaceId,
      callerAgentId: ids[0],
      title: "检查回调参数",
      model: "offline-model",
    });
    const child = await client.createAgent({
      provider: "codex",
      cwd: workspace.repoPath,
      workspaceId: workspace.workspaceId,
      callerAgentId: ids[0],
      title: "补充回归测试",
      model: "offline-model",
    });
    await client.sendAgentMessage(child.id, "permit-command");
    const card = (index: number) => page.getByTestId(`task-card-${getServerId()}-${ids[index]}`);
    await page.goto("/tasks", { waitUntil: "commit" });
    await expect(page.getByTestId("task-board")).toBeVisible();
    await expect(page.getByTestId("task-lane-needs_action").getByTestId(/^task-card-/)).toHaveCount(
      2,
    );
    await expect(page.getByTestId("task-lane-running").getByTestId(/^task-card-/)).toHaveCount(3);
    await expect(page.getByTestId("task-lane-review").getByTestId(/^task-card-/)).toHaveCount(2);
    await expect(page.getByTestId("task-lane-done").getByTestId(/^task-card-/)).toHaveCount(2);
    await card(0).click();
    await expect(page.getByRole("button", { name: "Allow once", exact: true })).toBeVisible();
    await expect(page.getByTestId("task-mark-read")).toHaveCount(0);
    await expect(page.getByTestId(`task-child-${child.id}`)).toBeVisible();
    await expect(page.getByTestId("task-detail")).toContainText("回调参数校验已修复");
    await page.screenshot({ path: testInfo.outputPath("task-board-desktop.png"), fullPage: true });

    await page.getByTestId("task-board-search").fill("发布");
    await expect(page.getByTestId(/^task-card-/)).toHaveCount(1);
    await expect(card(4)).toBeVisible();
    await page.getByTestId("task-board-search").fill("no-matching-task");
    await expect(page.getByTestId(/^task-card-/)).toHaveCount(0);
    await page.getByTestId("task-board-search").fill("");
    await expect(page.getByTestId(/^task-card-/)).toHaveCount(9);
    await page.getByRole("button", { name: "Allow once", exact: true }).click();
    await expect(page.getByRole("button", { name: "Allow once", exact: true })).toHaveCount(0);
    await expect
      .poll(async () => (await client.fetchAgent(ids[0]))?.agent.pendingPermissions.length)
      .toBe(0);

    await card(2).click();
    await page.getByTestId("task-stop").click();
    await expect.poll(async () => (await client.fetchAgent(ids[2]))?.agent.status).toBe("idle");
    await card(5).click();
    await page.getByTestId("task-mark-read").click();
    await expect(
      page.getByTestId("task-lane-done").getByTestId(`task-card-${getServerId()}-${ids[5]}`),
    ).toBeVisible();
    await card(7).click();
    await page.getByTestId("task-archive").click();
    await expect(card(7)).toHaveCount(0);
    await expect
      .poll(async () => Boolean((await client.fetchAgent(ids[7]))?.agent.archivedAt))
      .toBe(true);
    await card(8).click();
    await page.getByTestId("task-open-conversation").click();
    await expect(page).toHaveURL(/\/workspace\//);
    expect(errors).toEqual([]);
  });

  test("keeps parent and child tasks navigable on a narrow screen", async ({ page }, testInfo) => {
    const client = workspace.client as unknown as DaemonClient;
    const parent = await client.createAgent({
      provider: "codex",
      cwd: workspace.repoPath,
      workspaceId: workspace.workspaceId,
      title: "修复登录回调",
      model: "offline-model",
    });
    const child = await client.createAgent({
      provider: "codex",
      cwd: workspace.repoPath,
      workspaceId: workspace.workspaceId,
      callerAgentId: parent.id,
      title: "补充回归测试",
      model: "offline-model",
    });
    await client.sendAgentMessage(child.id, "permit-command");
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/tasks", { waitUntil: "commit" });
    const parentCard = page.getByTestId(`task-card-${getServerId()}-${parent.id}`);
    await expect(parentCard).toBeVisible();
    await expect(page.getByTestId(/^task-card-/)).toHaveCount(1);
    await parentCard.click();
    await expect(page.getByTestId("task-detail-sheet")).toBeVisible();
    await page.getByTestId(`task-child-${child.id}`).click();
    await expect(page.getByTestId("permission-request-deny")).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath("task-board-mobile.png"), fullPage: true });
    await page.getByTestId("permission-request-deny").click();
    await expect
      .poll(async () => (await client.fetchAgent(child.id))?.agent.pendingPermissions.length)
      .toBe(0);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
      true,
    );
    expect(errors).toEqual([]);
  });
});

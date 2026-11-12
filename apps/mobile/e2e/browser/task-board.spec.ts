import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { expect, test } from "../support/fixtures";
import { seedWorkspace, type SeededWorkspace } from "../support/helpers/seed-client";
import { getServerId } from "../support/helpers/server-id";
import { buildSeededHost } from "../support/helpers/daemon-registry";

test.describe("Task overview", () => {
  let workspace: SeededWorkspace;
  const errors: string[] = [];
  test.beforeEach(async ({ page, colorScheme }) => {
    errors.length = 0;
    page.on("pageerror", (error) => errors.push(error.message));
    await page.addInitScript(
      (theme) =>
        localStorage.setItem("@paseo:app-settings", JSON.stringify({ theme, language: "zh-CN" })),
      colorScheme === "dark" ? "dark" : "light",
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
      1,
    );
    await expect(page.getByTestId("task-lane-running").getByTestId(/^task-card-/)).toHaveCount(3);
    await expect(page.getByTestId("task-lane-review").getByTestId(/^task-card-/)).toHaveCount(3);
    await expect(page.getByTestId("task-lane-done").getByTestId(/^task-card-/)).toHaveCount(2);
    await expect(page.getByTestId("sidebar-task-counts")).toContainText("1");
    await expect(card(0).getByRole("button", { name: "Allow once", exact: true })).toBeVisible();
    await expect(card(2).getByTestId("task-inline-stop")).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath("task-board-inline-desktop.png") });
    await page.getByTestId("task-collapse-running").click();
    await expect(page.getByTestId("task-lane-running").getByTestId(/^task-card-/)).toHaveCount(0);
    const collapsed = await page.getByTestId("task-lane-running").boundingBox();
    expect(collapsed!.width).toBeGreaterThanOrEqual(40);
    expect(collapsed!.width).toBeLessThanOrEqual(44);
    await page.screenshot({ path: testInfo.outputPath("task-board-collapsed.png") });
    await page.getByTestId("task-expand-running").click();
    await expect(card(2)).toBeVisible();
    await card(0).getByTestId("task-inline-children").click();
    await expect(card(0).getByTestId(`task-inline-child-${child.id}`)).toBeVisible();
    await page.getByTestId("task-board-subtasks").click();
    await expect(card(0).getByTestId("task-inline-children")).toHaveCount(0);
    await page.getByTestId("task-board-subtasks").click();
    await card(0).getByTestId("task-summary").click();
    await expect(
      page.getByTestId("task-detail").getByRole("button", { name: "Allow once", exact: true }),
    ).toBeVisible();
    await expect(page.getByTestId("task-mark-read")).toHaveCount(0);
    await expect(page.getByTestId(`task-child-${child.id}`)).toBeVisible();
    await expect(page.getByTestId("task-detail")).toContainText("回调参数校验已修复");
    const toolbar = await page.getByTestId("task-board-toolbar").boundingBox();
    const detailHeader = await page.getByTestId("task-detail-header").boundingBox();
    expect(
      Math.abs(detailHeader!.y + detailHeader!.height - (toolbar!.y + toolbar!.height)),
    ).toBeLessThanOrEqual(1);
    await page.screenshot({ path: testInfo.outputPath("task-board-desktop.png"), fullPage: true });

    await page.getByTestId("task-detail-close").click();
    await page.getByTestId("task-board-search-toggle").click();
    await page.getByTestId("task-board-search").fill("发布");
    await expect(page.getByTestId(/^task-card-/)).toHaveCount(1);
    await expect(card(4)).toBeVisible();
    await page.getByTestId("task-board-search").fill("no-matching-task");
    await expect(page.getByTestId(/^task-card-/)).toHaveCount(0);
    await page.getByTestId("task-board-search").fill("");
    await expect(page.getByTestId(/^task-card-/)).toHaveCount(9);
    await card(0).getByRole("button", { name: "Allow once", exact: true }).click();
    await expect(card(0).getByRole("button", { name: "Allow once", exact: true })).toHaveCount(0);
    await expect(page.getByTestId("task-detail")).toHaveCount(0);
    await expect
      .poll(async () => (await client.fetchAgent(ids[0]))?.agent.pendingPermissions.length)
      .toBe(0);

    await card(2).getByTestId("task-inline-stop").click();
    await expect.poll(async () => (await client.fetchAgent(ids[2]))?.agent.status).toBe("idle");
    await card(5).getByTestId("task-inline-mark-read").click();
    await expect(
      page.getByTestId("task-lane-done").getByTestId(`task-card-${getServerId()}-${ids[5]}`),
    ).toBeVisible();
    await card(7).getByTestId("task-inline-archive").click();
    await expect(card(7)).toHaveCount(0);
    await expect
      .poll(async () => Boolean((await client.fetchAgent(ids[7]))?.agent.archivedAt))
      .toBe(true);
    await card(8).getByTestId("task-inline-open").click();
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
    await page.screenshot({
      path: testInfo.outputPath("task-board-inline-mobile.png"),
      fullPage: true,
    });
    await parentCard.getByTestId("task-inline-children").click();
    await parentCard.getByTestId(`task-inline-child-${child.id}`).click();
    await expect(page.getByTestId("task-detail-sheet")).toBeVisible();
    await page
      .getByTestId("task-detail")
      .getByRole("button", { name: "修复登录回调", exact: true })
      .click();
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

  test("keeps inline actions and question answers usable across tablet and phone layouts", async ({
    page,
  }, testInfo) => {
    const client = workspace.client as unknown as DaemonClient;
    const specs = [
      ["运行任务看板回归测试", "permit-command"],
      ["处理乱序的任务状态更新", "permit-file"],
      ["选择平板上的详情布局", "permit-question"],
      ["保持任务标题稳定", "stream"],
      ["修复窄屏详情面板", "stream"],
      ["优化移动端列表渲染", "stream"],
      ["防止卡片标题更新时跳动", "已完成标题布局修复。"],
      ["完善错误摘要", "已完成错误摘要。"],
      ["扩展工具栏搜索", "已完成搜索。"],
    ];
    const ids: string[] = [];
    for (const [title, prompt] of specs) {
      const agent = await client.createAgent({
        provider: "codex",
        cwd: workspace.repoPath,
        workspaceId: workspace.workspaceId,
        title,
        model: "offline-model",
        thinkingOptionId: "high",
      });
      ids.push(agent.id);
      await client.sendAgentMessage(agent.id, prompt);
      if (ids.length > 6) await client.waitForFinish(agent.id);
    }
    await page.setViewportSize({ width: 1200, height: 820 });
    await page.goto("/tasks", { waitUntil: "commit" });
    const card = (index: number) => page.getByTestId(`task-card-${getServerId()}-${ids[index]}`);
    await expect(card(2).getByTestId("task-answer")).toBeVisible();
    await expect(card(2)).toContainText("Select one");
    await expect(card(0)).toContainText("high");
    await page.getByTestId("task-collapse-running").click();
    await expect(page.getByTestId("task-expand-running")).toBeVisible();
    const doneLane = await page.getByTestId("task-lane-done").boundingBox();
    expect(doneLane!.x + doneLane!.width).toBeLessThanOrEqual(1200);
    const reviewOpen = await card(6).getByTestId("task-inline-open").boundingBox();
    expect(reviewOpen!.height).toBeLessThanOrEqual(32);
    // Unbroken permission input (JSON, paths) wraps inside the card instead of being clipped.
    const overflowing = await card(1).evaluate((element) => {
      const right = element.getBoundingClientRect().right;
      return [...element.querySelectorAll("*")].filter(
        (child) => child.getBoundingClientRect().right > right + 1,
      ).length;
    });
    expect(overflowing).toBe(0);
    await page.screenshot({
      path: testInfo.outputPath("task-board-reference-tablet.png"),
      fullPage: true,
    });

    await page.getByTestId("task-board-filters").click();
    await expect(page.getByTestId("task-board-filter-sheet")).toBeVisible();
    await page.getByTestId("task-board-sort").click();
    await page.getByText("最早活跃", { exact: true }).last().click();
    await page.getByRole("button", { name: "显示 9 个任务", exact: true }).click();
    await expect(page.getByTestId("task-board-filters")).toHaveAttribute("aria-pressed", "true");

    await page.setViewportSize({ width: 390, height: 844 });
    await page.getByTestId("task-tab-running").click();
    await expect(page.getByTestId("task-inline-stop")).toHaveCount(3);
    await expect(page.getByTestId("task-lane-running")).toBeVisible();
    await page.screenshot({
      path: testInfo.outputPath("task-board-reference-phone.png"),
      fullPage: true,
    });
    await page.getByTestId("task-board-search-toggle").click();
    const search = page.getByTestId("task-board-search");
    await search.fill("优化");
    await expect(page.getByTestId("task-inline-stop")).toHaveCount(1);
    const searchBounds = await search.boundingBox();
    const filterBounds = await page.getByTestId("task-board-filters").boundingBox();
    expect(searchBounds!.width).toBeGreaterThanOrEqual(120);
    expect(searchBounds!.x + searchBounds!.width).toBeLessThanOrEqual(filterBounds!.x);
    await search.fill("");
    await expect(page.getByTestId("task-inline-stop")).toHaveCount(3);
    await card(3).getByTestId("task-inline-stop").click();
    await expect.poll(async () => (await client.fetchAgent(ids[3]))?.agent.status).toBe("idle");
    await expect(page.getByTestId("task-detail")).toHaveCount(0);
    await page.getByTestId("task-tab-needs_action").click();
    await card(2).getByTestId("task-answer").click();
    const form = page.getByTestId("question-form-card");
    await expect(form).toBeVisible();
    expect((await client.fetchAgent(ids[2]))?.agent.pendingPermissions).toHaveLength(1);
    await form.getByRole("radio", { name: "first", exact: true }).click();
    await form.getByTestId("question-form-primary-action").click();
    await expect
      .poll(async () => (await client.fetchAgent(ids[2]))?.agent.pendingPermissions.length)
      .toBe(0);
    await expect(form).toHaveCount(0);
    await page
      .getByTestId("task-detail-sheet")
      .getByRole("button", { name: "关闭", exact: true })
      .click();
    await page.setViewportSize({ width: 1200, height: 820 });
    for (const lane of ["needs_action", "review", "done"]) {
      await page.getByTestId(`task-collapse-${lane}`).click();
    }
    await page.setViewportSize({ width: 650, height: 844 });
    await card(0).getByTestId("task-summary").click();
    await expect(page.getByTestId("task-detail-sheet")).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
      true,
    );
    expect(errors).toEqual([]);
  });

  test.describe("dark desktop layout", () => {
    test.use({ colorScheme: "dark" });

    test("contains long task cards, preserves all lanes beside details and identifies unavailable hosts", async ({
      page,
    }, testInfo) => {
      const client = workspace.client as unknown as DaemonClient;
      await client.renameProject(
        workspace.projectId,
        "ait-provider-integration-and-workspace-observation",
      );
      const ids: string[] = [];
      for (let index = 0; index < 32; index += 1) {
        const agent = await client.createAgent({
          provider: "codex",
          cwd: workspace.repoPath,
          workspaceId: workspace.workspaceId,
          title: `${index + 1}. Review provider-compatibility-and-task-directory-sync-regression`,
          model: "offline-model",
        });
        ids.push(agent.id);
      }
      await client.sendAgentMessage(ids[0], "permit-command");
      await client.sendAgentMessage(ids[1], "stream");
      await client.sendAgentMessage(ids[2], "任务已完成，等待查看结果。");
      await client.waitForFinish(ids[2]);
      await page.setViewportSize({ width: 2048, height: 1152 });
      await page.goto("/tasks", { waitUntil: "commit" });
      await expect(page.getByTestId("sidebar-sessions")).toBeVisible();
      const card = page.getByTestId(`task-card-${getServerId()}-${ids[0]}`);
      await expect(card).toBeVisible();

      const expectLanesToFit = async (withDetail = false) => {
        await expect
          .poll(async () => {
            const board = await page.getByTestId("task-board").boundingBox();
            const detail = withDetail ? await page.getByTestId("task-detail").boundingBox() : null;
            const lanes = await page.getByTestId(/^task-lane-/).evaluateAll((nodes) =>
              nodes.map((node) => {
                const { left, right, width } = node.getBoundingClientRect();
                return { left, right, width };
              }),
            );
            if (!board || lanes.length !== 4 || (withDetail && !detail)) return false;
            return (
              lanes.every(
                (lane) =>
                  lane.left >= board.x &&
                  lane.right <= (detail?.x ?? board.x + board.width) + 1 &&
                  lane.width >= 240 &&
                  Math.abs(lane.width - lanes[0].width) <= 1,
              ) &&
              (!detail || detail.x + detail.width <= board.x + board.width + 1)
            );
          })
          .toBe(true);
      };

      await page.screenshot({ path: testInfo.outputPath("task-board-long-content.png") });
      await expectLanesToFit();
      await card.getByTestId("task-summary").click();
      await expect(page.getByTestId("task-detail")).toBeVisible();
      await expectLanesToFit(true);
      await expect
        .poll(async () => {
          const detail = await page.getByTestId("task-detail").boundingBox();
          const project = await page
            .getByTestId("task-detail")
            .getByText("ait-provider-integration-and-workspace-observation › Ait", { exact: true })
            .boundingBox();
          return detail && project ? project.x - detail.x : 0;
        })
        .toBeGreaterThanOrEqual(12);
      await page.setViewportSize({ width: 1717, height: 916 });
      await expectLanesToFit(true);

      await page.setViewportSize({ width: 1600, height: 916 });
      await expect(page.getByTestId("task-detail-sheet")).toBeVisible();
      await page.setViewportSize({ width: 2048, height: 1152 });
      await expect(page.getByTestId("task-detail-sheet")).toHaveCount(0);
      await expectLanesToFit(true);

      const offlinePort = 47831;
      await page.route(new RegExp(`127\\.0\\.0\\.1:${offlinePort}`), (route) => route.abort());
      await page.routeWebSocket(new RegExp(`127\\.0\\.0\\.1:${offlinePort}`), (socket) =>
        socket.close(),
      );
      const offlineHost = buildSeededHost({
        serverId: "11111111-2222-4333-8444-555555555555",
        endpoint: `127.0.0.1:${offlinePort}`,
        label: "Offline demo host",
        nowIso: new Date().toISOString(),
      });
      await page.evaluate((host) => {
        localStorage.setItem("@paseo:e2e-extra-hosts", JSON.stringify([host]));
      }, offlineHost);
      await page.reload();
      await expect(card).toBeVisible();
      await expect(page.getByTestId("task-board-notice")).toContainText("Offline demo host");
      await expect(page.getByTestId("task-board-notice")).not.toContainText("未能加载任务");
      await card.getByTestId("task-summary").click();
      await expectLanesToFit(true);
      await page.screenshot({ path: testInfo.outputPath("task-board-dark-desktop.png") });

      await page.getByTestId("task-detail-close").click();
      await page.setViewportSize({ width: 390, height: 844 });
      await page.getByTestId("task-tab-done").click();
      const lane = await page.getByTestId("task-lane-done").boundingBox();
      expect(lane!.x + lane!.width).toBeLessThanOrEqual(390);
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
        true,
      );
      expect(errors).toEqual([]);
    });
  });
});

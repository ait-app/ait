import { expect, test } from "../support/fixtures";
import { gotoWorkspace } from "../support/helpers/launcher";
import { startIsolatedHostDaemon } from "../support/helpers/isolated-host-daemon";
import { addScheduleHostAndReload as addHostAndReload } from "../support/helpers/schedule-host";
import { seedWorkspace } from "../support/helpers/seed-client";

test("host monitor reads real daemon counters and opens beside the sidebar", async ({
  page,
}, testInfo) => {
  const workspace = await seedWorkspace({ repoPrefix: "host-monitor-", title: "Host monitor" });
  try {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.addInitScript(() => {
      const settings = JSON.parse(localStorage.getItem("@paseo:app-settings") ?? "{}");
      localStorage.setItem(
        "@paseo:app-settings",
        JSON.stringify({ ...settings, language: "zh-CN" }),
      );
    });
    await gotoWorkspace(page, workspace.workspaceId);
    const trigger = page.getByTestId("sidebar-host-monitor").filter({ visible: true });
    await expect(trigger).toBeVisible();
    await expect(trigger).toContainText(/CPU\s+\d/);
    await trigger.click();
    const details = page.getByTestId("host-monitor-details").filter({ visible: true });
    await expect(details).toBeVisible();
    await expect(details).toContainText("linux");
    await expect(details).toContainText(/GiB|MiB/);
    await expect(details).toContainText(/Last sample|最后采样/);
    const sidebarBounds = await trigger.boundingBox();
    const detailBounds = await details.boundingBox();
    expect(detailBounds!.x).toBeGreaterThan(sidebarBounds!.x);
    await expect(
      details.getByTestId("host-monitor-history").first().locator("svg path"),
    ).toHaveAttribute("d", /L/);
    await expect(details.getByText(/最近 (1[0-9]|[2-9][0-9]) 秒/)).toBeVisible({ timeout: 20_000 });
    await page.screenshot({
      path: testInfo.outputPath("host-monitor-desktop.png"),
      fullPage: true,
    });
    await page.keyboard.press("Escape");
    await expect(details).not.toBeVisible();
    await expect(trigger).toBeVisible();
    await page.evaluate(() => {
      const settings = JSON.parse(localStorage.getItem("@paseo:app-settings") ?? "{}");
      localStorage.setItem("@paseo:app-settings", JSON.stringify({ ...settings, theme: "dark" }));
    });
    await page.reload();
    await expect(trigger).toContainText(/CPU\s+\d/);
    await trigger.click();
    await expect(details).toBeVisible();
    await expect(details.getByText("实时", { exact: true })).toBeVisible();
    await expect(
      details.getByTestId("host-monitor-history").first().locator("svg path"),
    ).toHaveAttribute("d", /L/);
    await expect(details).toHaveCSS("opacity", "1");
    await page.screenshot({
      animations: "disabled",
      path: testInfo.outputPath("host-monitor-desktop-dark.png"),
      fullPage: true,
    });
  } finally {
    await workspace.cleanup();
  }
});

test("host selection stays pinned while offline and can resume following the workspace", async ({
  page,
}) => {
  const workspace = await seedWorkspace({ repoPrefix: "host-monitor-multiple-" });
  const secondary = await startIsolatedHostDaemon("monitor-secondary");
  try {
    await gotoWorkspace(page, workspace.workspaceId);
    await addHostAndReload({
      page,
      serverId: secondary.serverId,
      port: secondary.port,
      label: "Monitor secondary",
    });
    const trigger = page.getByTestId("sidebar-host-monitor").filter({ visible: true });
    await trigger.click();
    const detail = page.getByTestId("host-monitor-details").filter({ visible: true });
    await detail.getByRole("button", { name: "Monitor host", exact: true }).click();
    await page
      .getByTestId("combobox-desktop-container")
      .getByText("Monitor secondary", { exact: true })
      .click();
    await expect(trigger).toContainText("Monitor secondary");
    await expect(trigger).toContainText("Pinned host");
    await expect(detail).toContainText("linux");
    await secondary.close();
    await expect(trigger).toContainText("Host offline");
    await expect(trigger).toContainText("Monitor secondary");
    await expect(trigger).not.toContainText(/CPU\s+\d/);
    await expect(detail).toContainText("Last sample");
    await detail.getByRole("button", { name: "Monitor host", exact: true }).click();
    await page
      .getByTestId("combobox-desktop-container")
      .getByText("Follow current workspace", { exact: true })
      .click();
    await expect(trigger).toContainText("Workspace host");
    await expect(trigger).not.toContainText("Monitor secondary");
    await expect(trigger).toContainText(/CPU\s+\d/);
  } finally {
    await secondary.close();
    await workspace.cleanup();
  }
});

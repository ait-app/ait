import type { Locator } from "@playwright/test";
import { expect, test } from "../support/fixtures";
import { gotoAppShell } from "../support/helpers/app";
import { seedWorkspace } from "../support/helpers/seed-client";
import { getServerId } from "../support/helpers/server-id";

async function expectPinBesideTitle(row: Locator, title: string, pin: Locator, truncated: boolean) {
  await expect(pin).toBeVisible();
  const text = row.getByText(title, { exact: true });
  const titleBox = (await text.boundingBox())!;
  const pinBox = (await pin.boundingBox())!;
  const rowBox = (await row.boundingBox())!;
  expect(pinBox.width).toBe(12);
  expect(pinBox.x - (titleBox.x + titleBox.width)).toBeCloseTo(4, 0);
  expect(pinBox.x + pinBox.width).toBeLessThanOrEqual(rowBox.x + rowBox.width);
  expect(await text.evaluate((element) => element.scrollWidth > element.clientWidth)).toBe(
    truncated,
  );
  return pinBox.x;
}

test("keeps pins beside short titles and visible after truncated titles, including on hover", async ({
  page,
}, testInfo) => {
  const shortTitle = "Fix login";
  const longTitle =
    "Implement session pinning with a very long title that must truncate before the pin";
  const shortProject = "Ait Desktop";
  const longProject = "Workspace with a very long title that must keep its pin visible";
  const short = await seedWorkspace({ repoPrefix: "pin-short-", title: shortTitle });
  const long = await seedWorkspace({ repoPrefix: "pin-long-", title: longTitle });
  try {
    await page.setViewportSize({ width: 1280, height: 800 });
    await short.client.renameProject(short.projectId, shortProject);
    await long.client.renameProject(long.projectId, longProject);
    await short.client.setWorkspacePinned(short.workspaceId, true);
    await long.client.setWorkspacePinned(long.workspaceId, true);
    await gotoAppShell(page);
    for (const name of [longProject, shortProject]) {
      const project = page
        .locator('[data-testid^="sidebar-project-row-"]')
        .filter({ hasText: name });
      await project.click({ button: "right" });
      await page.locator('[data-testid^="sidebar-project-menu-pin-"]').click();
    }
    for (const [workspace, title, projectTitle, truncated] of [
      [short, shortTitle, shortProject, false],
      [long, longTitle, longProject, true],
    ] as const) {
      const project = page
        .locator('[data-testid^="sidebar-project-row-"]')
        .filter({ hasText: projectTitle });
      const projectPin = project.locator('[data-testid^="sidebar-project-pin-indicator-"]');
      const row = page.getByTestId(
        `sidebar-workspace-row-${getServerId()}:${workspace.workspaceId}`,
      );
      const pin = row.locator('[data-testid^="sidebar-workspace-pin-indicator-"]');
      await page.mouse.move(700, 500);
      const projectPinX = await expectPinBesideTitle(project, projectTitle, projectPin, truncated);
      await project.hover();
      expect(await expectPinBesideTitle(project, projectTitle, projectPin, truncated)).toBeCloseTo(
        projectPinX,
        0,
      );
      await page.mouse.move(700, 500);
      const pinX = await expectPinBesideTitle(row, title, pin, truncated);
      await row.hover();
      expect(await expectPinBesideTitle(row, title, pin, truncated)).toBeCloseTo(pinX, 0);
      const scrim = row.getByTestId("sidebar-workspace-trailing-scrim");
      await expect(scrim).toBeVisible();
      expect((await scrim.boundingBox())!.x).toBeGreaterThanOrEqual(pinX + 12);
    }
    await page.screenshot({
      path: testInfo.outputPath("pins-long-title-hover.png"),
      clip: { x: 0, y: 0, width: 320, height: 740 },
    });
    await page.mouse.move(700, 500);
    await expect(page.getByTestId("workspace-hover-card")).toHaveCount(0);
    await page.screenshot({
      path: testInfo.outputPath("pins-short-and-long-titles.png"),
      clip: { x: 0, y: 0, width: 320, height: 740 },
    });
  } finally {
    await short.cleanup();
    await long.cleanup();
  }
});

import { existsSync } from "node:fs";
import path from "node:path";
import { expect, test } from "../support/fixtures";
import { gotoAppShell } from "../support/helpers/app";
import { seedWorkspace } from "../support/helpers/seed-client";
import {
  expectWorkspaceAbsentFromSidebar,
  selectWorkspaceInSidebar,
} from "../support/helpers/sidebar";
import { expectWorkspaceHeader, waitForSidebarHydration } from "../support/helpers/workspace-ui";

test.describe("Workspace header", () => {
  test("leads from the project to the workspace and archives from its menu", async ({ page }) => {
    const workspace = await seedWorkspace({
      repoPrefix: "header-archive-",
      title: "Header archive",
    });

    try {
      await gotoAppShell(page);
      await waitForSidebarHydration(page);
      await selectWorkspaceInSidebar(page, workspace.workspaceId);

      await expectWorkspaceHeader(page, {
        title: workspace.workspaceName,
        subtitle: path.basename(workspace.repoPath),
      });
      const separator = page.getByTestId("workspace-header-breadcrumb-separator");
      await expect(separator).toBeVisible();
      const [projectBox, separatorBox, titleBox] = await Promise.all([
        page.getByTestId("workspace-header-subtitle").boundingBox(),
        separator.boundingBox(),
        page.getByTestId("workspace-header-title").boundingBox(),
      ]);
      expect(projectBox!.x + projectBox!.width).toBeLessThanOrEqual(separatorBox!.x);
      expect(separatorBox!.x + separatorBox!.width).toBeLessThanOrEqual(titleBox!.x);

      // Archive is a workspace action, so the Git menu on the right no longer offers it.
      await page.getByTestId("changes-primary-cta-caret").click();
      await expect(page.getByTestId("changes-primary-cta-menu")).toBeVisible();
      await expect(page.getByTestId("workspace-archive-action")).toHaveCount(0);
      await page.keyboard.press("Escape");

      await page.getByTestId("workspace-header-menu-trigger").click();
      await page.getByTestId("workspace-archive-action").click();

      await expectWorkspaceAbsentFromSidebar(page, workspace.workspaceId);
      expect(existsSync(workspace.repoPath)).toBe(true);
    } finally {
      await workspace.cleanup();
    }
  });

  test("compact layout lists terminal profiles in the tab switcher, not the workspace menu", async ({
    page,
  }) => {
    const workspace = await seedWorkspace({ repoPrefix: "header-profiles-" });

    try {
      await gotoAppShell(page);
      await waitForSidebarHydration(page);
      await selectWorkspaceInSidebar(page, workspace.workspaceId);
      await expect(page.getByTestId("workspace-header-title").first()).toBeVisible();
      await page.setViewportSize({ width: 390, height: 844 });

      await page.getByTestId("workspace-tab-switcher-trigger").click();
      const profiles = page.getByTestId("workspace-tab-switcher-terminal-profiles");
      await expect(profiles).toBeInViewport({ ratio: 1 });
      await expect(profiles.getByText("Terminal profiles", { exact: true })).toBeVisible();
      await expect(
        page.getByTestId("workspace-tab-switcher-terminal-profile-claude"),
      ).toBeVisible();
      await expect(page.getByTestId("workspace-tab-switcher-edit-terminal-profiles")).toBeVisible();
      await expect(page.getByTestId("workspace-tab-switcher-new-terminal")).toBeInViewport({
        ratio: 1,
      });
      const backdrop = page.getByRole("button", { name: "Bottom sheet backdrop" }).first();
      await expect(async () => {
        if (await backdrop.isVisible()) {
          await backdrop.click({ position: { x: 195, y: 24 } });
        }
        await expect(backdrop).not.toBeVisible({ timeout: 1_000 });
      }).toPass({ timeout: 15_000 });

      await page.getByTestId("workspace-header-menu-trigger").filter({ visible: true }).click();
      await expect(page.getByTestId("workspace-archive-action")).toBeVisible();
      // The New tab page behind the sheet has its own "Terminal profiles" heading, so the menu
      // is checked by the item it used to end with.
      await expect(page.getByTestId("workspace-header-edit-terminal-profiles")).toHaveCount(0);
      await expect(page.getByRole("menuitem", { name: "Edit profiles" })).toHaveCount(0);
    } finally {
      await workspace.cleanup();
    }
  });
});

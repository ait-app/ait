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
      await page.getByTestId("changes-actions-menu-trigger").click();
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
});

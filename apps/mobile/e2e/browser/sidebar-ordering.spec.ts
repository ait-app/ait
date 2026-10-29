import type { Locator } from "@playwright/test";
import { expect, test } from "../support/fixtures";
import { gotoAppShell } from "../support/helpers/app";
import { projectEquivalenceViewKey } from "../support/helpers/project-view-key";
import { getServerId } from "../support/helpers/server-id";
import { seedWorkspace } from "../support/helpers/seed-client";
import { waitForSidebarHydration } from "../support/helpers/workspace-ui";

async function rowTestIds(rows: Locator) {
  return rows.evaluateAll((elements) =>
    elements.map((element) => element.getAttribute("data-testid")),
  );
}

async function attemptDragFirstRowAfterSecond(rows: Locator) {
  await expect(rows).toHaveCount(2);
  const before = await rowTestIds(rows);
  const source = await rows.nth(0).boundingBox();
  const target = await rows.nth(1).boundingBox();
  if (!source || !target) throw new Error("Expected visible sidebar rows");

  const page = rows.page();
  // Use the title area, away from the menu and archive buttons.
  const x = source.x + source.width / 3;
  await page.mouse.move(x, source.y + source.height / 2);
  await page.mouse.down();
  await page.mouse.move(x, target.y + target.height / 2, { steps: 6 });
  await page.mouse.up();

  expect(await rowTestIds(rows)).toEqual(before);
  await expect(rows.locator('xpath=ancestor::*[@aria-roledescription="sortable"]')).toHaveCount(0);
}

test("project workspaces follow status time and sidebar rows cannot be manually reordered", async ({
  page,
}) => {
  const first = await seedWorkspace({ repoPrefix: "sidebar-ordering-first-", title: "A older" });
  const other = await seedWorkspace({ repoPrefix: "sidebar-ordering-second-" });

  try {
    const created = await first.client.createWorkspace({
      source: { kind: "directory", path: first.repoPath, projectId: first.projectId },
      title: "Z newer",
    });
    if (!created.workspace) throw new Error(created.error ?? "Failed to create second workspace");
    const newerId = created.workspace.id;
    const serverId = getServerId();
    const firstViewKey = projectEquivalenceViewKey(first.projectKey);
    const otherViewKey = projectEquivalenceViewKey(other.projectKey);
    const olderRow = `sidebar-workspace-row-${serverId}:${first.workspaceId}`;
    const newerRow = `sidebar-workspace-row-${serverId}:${newerId}`;
    const otherRow = `sidebar-workspace-row-${serverId}:${other.workspaceId}`;

    await gotoAppShell(page);
    await waitForSidebarHydration(page);
    // A pre-upgrade manual order must not override automatic sorting after reload.
    await page.evaluate(
      ({ firstViewKey, otherViewKey, serverId, olderId, newerId }) => {
        localStorage.setItem(
          "sidebar-project-workspace-order",
          JSON.stringify({
            version: 1,
            state: {
              projectOrder: [otherViewKey, firstViewKey],
              workspaceOrderByProject: {
                [firstViewKey]: [`${serverId}:${olderId}`, `${serverId}:${newerId}`],
              },
              pinnedWorkspaceOrder: [],
            },
          }),
        );
      },
      { firstViewKey, otherViewKey, serverId, olderId: first.workspaceId, newerId },
    );
    await page.reload();
    await waitForSidebarHydration(page);

    const projectRows = page.locator(
      `[data-testid="sidebar-project-row-${firstViewKey}"], [data-testid="sidebar-project-row-${otherViewKey}"]`,
    );
    await expect
      .poll(() => rowTestIds(projectRows))
      .toEqual([`sidebar-project-row-${firstViewKey}`, `sidebar-project-row-${otherViewKey}`]);
    await attemptDragFirstRowAfterSecond(projectRows);

    const workspaceRows = page.locator(`[data-testid="${olderRow}"], [data-testid="${newerRow}"]`);
    await expect.poll(() => rowTestIds(workspaceRows)).toEqual([newerRow, olderRow]);
    await attemptDragFirstRowAfterSecond(workspaceRows);

    await first.client.setWorkspacePinned(first.workspaceId, true);
    await other.client.setWorkspacePinned(other.workspaceId, true);
    const pinnedRows = page
      .getByTestId("sidebar-pinned-list")
      .locator(`[data-testid="${olderRow}"], [data-testid="${otherRow}"]`);
    await expect.poll(() => rowTestIds(pinnedRows)).toEqual([otherRow, olderRow]);
    await attemptDragFirstRowAfterSecond(pinnedRows);

    await page.evaluate(() => {
      const saved = JSON.parse(localStorage.getItem("sidebar-view") ?? "{}");
      localStorage.setItem(
        "sidebar-view",
        JSON.stringify({
          ...saved,
          state: { ...saved.state, groupMode: "status" },
        }),
      );
    });
    await page.reload();
    await expect(page.getByTestId("sidebar-status-list-scroll")).toBeVisible();
    await expect.poll(() => rowTestIds(pinnedRows)).toEqual([otherRow, olderRow]);
    await attemptDragFirstRowAfterSecond(pinnedRows);
  } finally {
    await first.cleanup();
    await other.cleanup();
  }
});

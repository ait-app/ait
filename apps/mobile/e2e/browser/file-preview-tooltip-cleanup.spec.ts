import { expect, test } from "../support/fixtures";
import { openFileFromExplorer } from "../support/helpers/file-explorer";
import { gotoWorkspace } from "../support/helpers/launcher";
import { seedWorkspace } from "../support/helpers/seed-client";
import { ensureExplorerSidebar } from "../support/helpers/workspace-tabs";

test("clears tooltips over a Markdown preview without relying on animation completion", async ({
  page,
}) => {
  const workspace = await seedWorkspace({
    repoPrefix: "file-preview-tooltip-",
    repo: { files: [{ path: "README.md", content: "# Preview content\n\nDocument body.\n" }] },
  });
  try {
    await gotoWorkspace(page, workspace.workspaceId);
    await ensureExplorerSidebar(page);
    await page.getByRole("button", { name: "Browse workspace files", exact: true }).click();
    await openFileFromExplorer(page, "README.md");
    await expect(page.getByTestId("markdown-preview-frame")).toContainText("Preview content");

    const trigger = page.getByTestId("workspace-explorer-toggle").filter({ visible: true }).first();
    const tooltip = page.getByTestId("workspace-explorer-toggle-tooltip");
    const body = page.getByText("Document body.", { exact: true });

    await trigger.hover();
    await expect(tooltip).toBeVisible();
    await expect(tooltip).toContainText("Toggle Explorer sidebar");
    await body.hover();
    await expect(tooltip).toHaveCount(0);

    // A browser can interrupt an animation before it dispatches its events.
    // The surface must still disappear, including any exit-animation clones.
    await page.addStyleTag({
      content: '[data-testid="workspace-explorer-toggle-tooltip"] { animation: none !important; }',
    });
    await trigger.hover();
    await expect(tooltip).toBeVisible();
    await body.hover();
    await expect(tooltip).toHaveCount(0);

    await trigger.hover();
    await expect(tooltip).toBeVisible();
    // Navigate while still hovered to cover unmount without a pointer-leave.
    await page.getByRole("button", { name: "New workspace", exact: true }).dispatchEvent("click");
    await expect(tooltip).toHaveCount(0);
  } finally {
    await workspace.cleanup();
  }
});

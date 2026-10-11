import { writeFile } from "node:fs/promises";
import path from "node:path";
import type { Page } from "@playwright/test";
import { resolveSyntaxColors } from "@ait/highlight";
import { APP_SETTINGS_KEY } from "../../src/hooks/use-settings/keys";
import { buildHostWorkspaceRoute } from "../../src/utils/host-routes";
import { test, expect } from "../support/fixtures";
import { getServerId } from "../support/helpers/server-id";
import { seedWorkspace } from "../support/helpers/seed-client";
import {
  ensureExplorerSidebar,
  waitForWorkspaceTabsVisible,
} from "../support/helpers/workspace-tabs";

test("an oversized file shows a warning while sibling diffs remain usable", async ({ page }) => {
  const workspace = await seedWorkspace({ repoPrefix: "ait-large-diff-file-" });
  try {
    await writeFile(path.join(workspace.repoPath, "large.txt"), "a\n".repeat(150_000));
    await writeFile(path.join(workspace.repoPath, "small.rs"), "fn small_sibling_change() {}\n");
    await page.addInitScript(
      ({ settingsKey }) => {
        localStorage.setItem(settingsKey, JSON.stringify({ theme: "light", syntaxTheme: "one" }));
        const painted = new Set<string>();
        const syntaxColors = new Set<string>();
        Object.assign(window, { diffPaintedText: painted, diffSyntaxColors: syntaxColors });
        const original = CanvasRenderingContext2D.prototype.fillText;
        CanvasRenderingContext2D.prototype.fillText = function (text, ...args) {
          painted.add(text);
          if (text.includes("small_sibling_change")) syntaxColors.add(String(this.fillStyle));
          return original.call(this, text, ...args);
        };
      },
      { settingsKey: APP_SETTINGS_KEY },
    );
    await page.setViewportSize({ width: 1400, height: 900 });
    await page.goto(buildHostWorkspaceRoute(getServerId(), workspace.workspaceId));
    await waitForWorkspaceTabsVisible(page);
    await openWorkingDiffTab(page);
    await expect(page.getByTestId("git-diff-canvas")).toBeVisible();
    await expect
      .poll(() =>
        page.evaluate((keywordColor) => {
          const text = (window as unknown as { diffPaintedText: Set<string> }).diffPaintedText;
          const colors = (window as unknown as { diffSyntaxColors: Set<string> }).diffSyntaxColors;
          return (
            text.has("Diff too large to display") &&
            text.has("fn small_sibling_change() {}") &&
            colors.has(keywordColor)
          );
        }, resolveSyntaxColors("one", "light").keyword),
      )
      .toBe(true);
    await expect(page.getByTestId("diff-too-large")).toHaveCount(0);
  } finally {
    await workspace.cleanup();
  }
});

test("oversized snapshots recover without reconnecting the host", async ({ page }, testInfo) => {
  const workspace = await seedWorkspace({ repoPrefix: "ait-large-diff-snapshot-" });
  let hostConnections = 0;
  let closedConnections = 0;
  page.on("websocket", (socket) => {
    if (socket.url().includes("/v1/ws")) {
      hostConnections++;
      socket.on("close", () => closedConnections++);
    }
  });
  try {
    await writeFile(path.join(workspace.repoPath, "first.txt"), "a\n".repeat(100_000));
    await writeFile(path.join(workspace.repoPath, "second.txt"), "b\n".repeat(100_000));
    const snapshot = await workspace.client.getCheckoutDiff(workspace.workspaceDirectory, {
      mode: "uncommitted",
    });
    const limitedFile = snapshot.files.find((file) => file.status === "too_large");
    expect(limitedFile).toBeDefined();
    expect(snapshot.files.some((file) => file.hunks.length > 0)).toBe(true);
    expect(snapshot.diffTooLarge).not.toBe(true);
    await page.addInitScript(() => {
      const painted = new Set<string>();
      Object.assign(window, { diffPaintedText: painted });
      const original = CanvasRenderingContext2D.prototype.fillText;
      CanvasRenderingContext2D.prototype.fillText = function (text, ...args) {
        painted.add(text);
        return original.call(this, text, ...args);
      };
    });
    await page.setViewportSize({ width: 1400, height: 900 });
    await page.goto(buildHostWorkspaceRoute(getServerId(), workspace.workspaceId));
    await waitForWorkspaceTabsVisible(page);
    const explorer = await ensureExplorerSidebar(page);
    await explorer.getByTestId("workspace-tab-changes_tree").click();
    const tree = page.getByTestId("changes-tree-panel");
    await expect(tree.getByTestId("diff-too-large")).toHaveCount(0);
    await expect(tree.getByText("first.txt", { exact: true })).toBeVisible();
    await expect(tree.getByText("second.txt", { exact: true })).toBeVisible();
    await openWorkingDiffTab(page);
    await tree
      .locator('[data-testid^="diff-tree-file-"][data-testid$="-toggle"]')
      .filter({ hasText: limitedFile!.path })
      .click();
    await expect(page.getByTestId("git-diff-canvas")).toBeVisible();
    await expect
      .poll(() =>
        page.evaluate(() =>
          (window as unknown as { diffPaintedText: Set<string> }).diffPaintedText.has(
            "Diff too large to display",
          ),
        ),
      )
      .toBe(true);
    const warning = page.getByTestId("working-diff-panel").getByTestId("diff-too-large");
    await expect(warning).toHaveCount(0);
    const screenshot = testInfo.outputPath("oversized-diff.png");
    await page.screenshot({ path: screenshot });
    await testInfo.attach("oversized-diff", { path: screenshot, contentType: "image/png" });

    expect(hostConnections).toBeGreaterThan(0);
    const previousHostConnections = hostConnections;
    const previousClosedConnections = closedConnections;
    await writeFile(path.join(workspace.repoPath, "first.txt"), "small\n");
    await writeFile(path.join(workspace.repoPath, "second.txt"), "small\n");
    await expect(warning).toHaveCount(0);
    await expect(page.getByTestId("git-diff-canvas")).toBeVisible();

    await writeFile(path.join(workspace.repoPath, "first.txt"), "a\n".repeat(1_500_000));
    await expect(warning).toBeVisible();
    await writeFile(path.join(workspace.repoPath, "first.txt"), "small again\n");
    await expect(warning).toHaveCount(0);
    await expect(page.getByTestId("git-diff-canvas")).toBeVisible();
    expect(hostConnections).toBe(previousHostConnections);
    expect(closedConnections).toBe(previousClosedConnections);
  } finally {
    await workspace.cleanup();
  }
});

async function openWorkingDiffTab(page: Page): Promise<void> {
  await page.getByTestId("workspace-new-tab-button").filter({ visible: true }).first().click();
  await page.getByTestId("workspace-new-tab-menu-diff").filter({ visible: true }).first().click();
  await expect(
    page.getByTestId("working-diff-panel").filter({ visible: true }).first(),
  ).toBeVisible();
}

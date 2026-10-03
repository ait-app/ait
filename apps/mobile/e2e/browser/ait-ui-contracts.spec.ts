import { test, expect } from "../support/fixtures";
import { seedWorkspace } from "../support/helpers/seed-client";
import { gotoWorkspace } from "../support/helpers/launcher";
import { createAgentTabFromMenu, openChangesTreePanel } from "../support/helpers/workspace-tabs";
import { getServerId } from "../support/helpers/server-id";
import { readFile, writeFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import path from "node:path";

for (const effort of ["max", "ultra"]) {
  test(`offline Codex UI preserves ${effort} through create and reload`, async ({ page }, info) => {
    const w = await seedWorkspace({ repoPrefix: `reasoning-${effort}-` });
    try {
      await writeFile(
        path.join(w.repoPath, "e2e-reasoning-options.json"),
        JSON.stringify(["high", "max", "ultra"]),
      );
      await gotoWorkspace(page, w.workspaceId);
      await createAgentTabFromMenu(page);
      await expect(
        page
          .getByRole("button", { name: "Select model (Offline model)", exact: true })
          .filter({ visible: true }),
      ).toBeVisible({ timeout: 30000 });
      await page
        .getByRole("button", { name: /Select thinking option/ })
        .filter({ visible: true })
        .click();
      await page
        .getByRole("dialog")
        .getByRole("button", { name: new RegExp(`^${effort}$`, "i") })
        .click();
      await page
        .getByRole("textbox", { name: "Message agent..." })
        .filter({ visible: true })
        .fill(`Verify ${effort} reasoning offline`);
      await page
        .getByRole("button", { name: "Send message", exact: true })
        .filter({ visible: true })
        .click();
      await expect(
        page.getByTestId("assistant-message").filter({ visible: true }).last(),
      ).toContainText(`Verify ${effort} reasoning offline`, { timeout: 30000 });
      const requests = (await readFile(path.join(w.repoPath, "native-requests.jsonl"), "utf8"))
        .trim()
        .split("\n")
        .map((raw) => JSON.parse(raw));
      expect(requests.findLast((r) => r.method === "turn/start").params.effort).toBe(effort);
      await page.reload();
      await expect(
        page
          .getByRole("button", { name: new RegExp(`Select thinking option.*${effort}`, "i") })
          .filter({ visible: true }),
      ).toBeVisible({ timeout: 30000 });
      await page.screenshot({ path: info.outputPath(`reasoning-${effort}.png`) });
    } finally {
      await w.cleanup();
    }
  });
}

test("sidebar committed diff clears when origin main moves without local main moving", async ({
  page,
}, info) => {
  const w = await seedWorkspace({
    repoPrefix: "sidebar-origin-",
    repo: { files: [{ path: "tracked.txt", content: "one\ntwo\n" }] },
  });
  const git = (...args: string[]) =>
    execFileSync("git", args, { cwd: w.repoPath, stdio: "ignore" });
  try {
    git("update-ref", "refs/remotes/origin/main", "main");
    git("checkout", "-b", "feature");
    await writeFile(path.join(w.repoPath, "tracked.txt"), "one\nthree\n");
    git("commit", "-am", "feature");
    await w.client.checkoutRefresh(w.repoPath);
    await gotoWorkspace(page, w.workspaceId);
    const row = page.getByTestId(`sidebar-workspace-row-${getServerId()}:${w.workspaceId}`).first();
    await expect(row).toContainText("+1");
    await expect(row).toContainText("-1");
    git("update-ref", "refs/remotes/origin/main", "HEAD");
    await w.client.checkoutRefresh(w.repoPath);
    await page.reload();
    await expect(row).toBeVisible({ timeout: 30000 });
    await expect(row).not.toContainText("+1");
    await expect(row).not.toContainText("-1");
    await openChangesTreePanel(page);
    await expect(
      page
        .locator('[data-testid^="diff-tree-file-"][data-testid$="-toggle"]')
        .filter({ visible: true }),
    ).toHaveCount(0);
    await page.screenshot({ path: info.outputPath("origin-base-cleared.png") });
  } finally {
    await w.cleanup();
  }
});

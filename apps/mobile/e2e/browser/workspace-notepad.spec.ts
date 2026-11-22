import { readdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { expect, test, type Page } from "../support/fixtures";
import { gotoWorkspace, openNewTabMenuWithShortcut } from "../support/helpers/launcher";
import { seedWorkspace } from "../support/helpers/seed-client";

const APP_SETTINGS_KEY = "@paseo:app-settings";

function editor(page: Page) {
  return page.getByTestId("file-source-editor").filter({ visible: true }).locator(".cm-content");
}

test.describe("Workspace notepad", () => {
  test("opens the host's notepad file in the file editor and follows other writers", async ({
    page,
  }) => {
    await page.addInitScript((settingsKey) => {
      localStorage.setItem(settingsKey, JSON.stringify({ experimentalFeatures: true }));
    }, APP_SETTINGS_KEY);
    const session = await seedWorkspace({ repoPrefix: "workspace-notepad-" });
    const dataDir = process.env.E2E_AIT_DATA_DIR;
    if (!dataDir) throw new Error("E2E_AIT_DATA_DIR is not set");
    // The notepad lives in the host's data directory, not in the checkout.
    const file = path.join(dataDir, "notepads", session.workspaceId, "notepad.md");
    const checkout = await readdir(session.repoPath);

    try {
      await gotoWorkspace(page, session.workspaceId);
      await openNewTabMenuWithShortcut(page);
      await page
        .getByTestId("workspace-new-tab-notebook")
        .filter({ visible: true })
        .first()
        .click();

      // The same pane as an opened file: editor first, with the Markdown preview one click away.
      await expect(page.getByTestId("workspace-file-pane").filter({ visible: true })).toBeVisible();
      await expect(editor(page)).toBeVisible();
      expect(await readFile(file, "utf8")).toBe("");

      await editor(page).fill("# Plan\n\n- [ ] ship the notepad");
      await expect.poll(() => readFile(file, "utf8")).toBe("# Plan\n\n- [ ] ship the notepad");

      // What an agent appends with notepad_write reaches the open editor without a reload.
      const appended = await session.client.writeWorkspaceNotepad({
        workspaceId: session.workspaceId,
        text: "Use port 7316",
        mode: "append",
      });
      expect(appended.accepted).toBe(true);
      await expect(editor(page)).toContainText("Use port 7316");
      await expect(editor(page)).toContainText("# Plan");

      // So does an edit made to the file with any other tool.
      await writeFile(file, "rewritten outside the app\n", "utf8");
      await expect(editor(page)).toContainText("rewritten outside the app");
      await expect(editor(page)).not.toContainText("# Plan");

      const preview = page
        .getByTestId("file-panel-bar")
        .filter({ visible: true })
        .getByRole("button", { name: "Preview", exact: true });
      await preview.click();
      await expect(preview).toHaveAttribute("aria-selected", "true");
      await expect(page.getByText("rewritten outside the app")).toBeVisible();

      const added = (await readdir(session.repoPath)).filter((name) => !checkout.includes(name));
      expect(added.filter((name) => /note(pad|book)/i.test(name))).toEqual([]);
    } finally {
      await session.cleanup();
    }
  });
});

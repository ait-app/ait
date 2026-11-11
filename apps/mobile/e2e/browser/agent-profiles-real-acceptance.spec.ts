import { expect, test } from "../support/fixtures";
import {
  agentProfileRow,
  applyProfileFromPicker,
  editAgentProfile,
  expectHostAgentProfiles,
  expectNoAgentProfiles,
  openAgentProfileSettings,
  openModelPicker,
  removeAgentProfile,
  seedAgentProfiles,
} from "../support/helpers/agent-profiles";
import { gotoAppShell } from "../support/helpers/app";
import {
  openGlobalNewWorkspaceComposer,
  selectNewWorkspaceProject,
} from "../support/helpers/new-workspace";
import { seedWorkspace } from "../support/helpers/seed-client";
import { waitForSidebarHydration } from "../support/helpers/workspace-ui";

// Recovery smoke: real daemon and its supported offline Codex stdio fixture.
// No browser transport interception or fabricated provider responses.
test("real profile create, edit, delete and compact Back", async ({ page }) => {
  const seed = await seedAgentProfiles([]);
  try {
    await openAgentProfileSettings(page);
    await expectNoAgentProfiles(page);
    await page.getByRole("button", { name: "New profile", exact: true }).click();
    const form = page.getByTestId("agent-profile-edit-page");
    await expect(form).toBeVisible();
    await form.getByRole("textbox", { name: "Name", exact: true }).fill("Real profile");
    await form.getByTestId("agent-profile-provider-trigger").click();
    await page
      .getByTestId("combobox-desktop-container")
      .getByRole("button", { name: /codex/i })
      .click();
    await form.getByRole("button", { name: "Save", exact: true }).click();
    await expect(agentProfileRow(page, "Real profile")).toBeVisible();
    await editAgentProfile(page, "Real profile", {
      name: "Edited real profile",
      notes: "Keep this note",
    });
    await expect(agentProfileRow(page, "Edited real profile")).toContainText("Keep this note");
    await page.setViewportSize({ width: 390, height: 844 });
    await agentProfileRow(page, "Edited real profile")
      .getByRole("button", { name: "Edit profile", exact: true })
      .click();
    await expect(form).toBeVisible();
    await page.screenshot({ path: `${process.env.TMPDIR}/task03-compact-editor.png` });
    await page.getByRole("button", { name: "Back", exact: true }).filter({ visible: true }).click();
    await expect(page.getByTestId("agent-profiles-card")).toBeVisible();
    await removeAgentProfile(page, "Edited real profile");
    await expectNoAgentProfiles(page);
    await page.getByRole("button", { name: "New profile", exact: true }).click();
    await expect(form).toBeVisible();
    await form.getByRole("textbox", { name: "Name", exact: true }).fill("Compact created profile");
    await form.getByTestId("agent-profile-provider-trigger").click();
    // Still at 390px: the combobox renders as a bottom sheet, so select the
    // option row itself rather than the desktop container.
    await page.getByTestId("agent-profile-provider-option-codex").click();
    await form.getByRole("button", { name: "Save", exact: true }).click();
    await expect(agentProfileRow(page, "Compact created profile")).toBeVisible();
  } finally {
    await seed.restore();
  }
});

test("real unknown-model save and clearing preserve future fields and other profiles", async ({
  page,
}) => {
  const stored = {
    id: "create",
    name: "Future profile",
    provider: "codex",
    model: "retired|opaque-model",
    futureSetting: { nested: ["keep", 42] },
  };
  const other = { id: "other", name: "Other profile", provider: "codex" };
  const seed = await seedAgentProfiles([stored, other]);
  try {
    await openAgentProfileSettings(page);
    await expect(page.getByTestId("agent-profile-unresolved-create")).toBeVisible();
    await agentProfileRow(page, stored.name)
      .getByRole("button", { name: "Edit profile", exact: true })
      .click();
    await expect(page).toHaveURL(/\/agent-profiles\/edit\/create$/);
    const form = page.getByTestId("agent-profile-edit-page");
    await expect(page.getByTestId("agent-profile-model-field")).toContainText(
      "Not offered by this host",
    );
    await page.screenshot({ path: `${process.env.TMPDIR}/task03-unresolved-editor.png` });
    await form.getByRole("button", { name: "Save", exact: true }).click();
    await expect(page.getByTestId("agent-profiles-card")).toBeVisible();
    await expectHostAgentProfiles([stored, other]);
    await agentProfileRow(page, stored.name)
      .getByRole("button", { name: "Edit profile", exact: true })
      .click();
    await form.getByTestId("agent-profile-model-trigger").click();
    await page.getByTestId("agent-profile-leave-unchanged-option").click();
    await form.getByRole("button", { name: "Save", exact: true }).click();
    await expect(page.getByTestId("agent-profiles-card")).toBeVisible();
    await expectHostAgentProfiles([
      {
        id: stored.id,
        name: stored.name,
        provider: stored.provider,
        futureSetting: stored.futureSetting,
      },
      other,
    ]);
  } finally {
    await seed.restore();
  }
});

test("real picker quick-apply and manage navigation use the same profile store", async ({
  page,
}) => {
  // `auto` is in Codex's real mode catalog; `read-only` is only the implicit
  // fallback when no mode is named, so a profile naming it would be dropped on
  // apply and prove nothing about the picker.
  const profile = {
    id: "real-picker",
    name: "Read-only work",
    provider: "codex",
    model: "offline-model",
    modeId: "auto",
  };
  const seed = await seedAgentProfiles([profile]);
  const workspace = await seedWorkspace({ repoPrefix: "profile-real-picker-" });
  try {
    await gotoAppShell(page);
    await waitForSidebarHydration(page);
    await openGlobalNewWorkspaceComposer(page);
    await selectNewWorkspaceProject(page, {
      projectKey: workspace.projectKey,
      projectDisplayName: workspace.projectDisplayName,
    });
    await openModelPicker(page);
    await expect(page.locator('[data-testid^="model-create-profile-"]')).toHaveCount(0);
    await expect(page.locator('[data-testid^="model-edit-profile-"]')).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Create profile", exact: true })).toHaveCount(0);
    await applyProfileFromPicker(page, profile.name);
    await expect
      .poll(() =>
        page.evaluate(() =>
          JSON.parse(localStorage.getItem("@paseo:create-agent-preferences") ?? "{}"),
        ),
      )
      .toMatchObject({
        provider: "codex",
        providerPreferences: { codex: { model: "offline-model", mode: "auto" } },
      });
    await openModelPicker(page);
    await page.getByRole("button", { name: "Edit agent profiles", exact: true }).click();
    await expect(page.getByTestId("agent-profiles-card")).toBeVisible();
    await expect(agentProfileRow(page, profile.name)).toBeVisible();
  } finally {
    await workspace.cleanup();
    await seed.restore();
  }
});

import { expect, test } from "../support/fixtures";
import {
  agentProfileRow,
  createAgentProfile,
  createAgentProfileFromEmptyState,
  editAgentProfile,
  expectAgentProfile,
  expectAgentProfileForm,
  expectAgentProfileOrder,
  expectAgentProfileTagsGone,
  expectHostAgentProfiles,
  expectNoAgentProfiles,
  moveAgentProfileUp,
  openAgentProfileSettings,
  removeAgentProfile,
  seedAgentProfiles,
  stageLegacyFavoritesForHostMigration,
} from "../support/helpers/agent-profiles";
import { expectAppRoute } from "../support/helpers/route-assertions";
import { getServerId } from "../support/helpers/server-id";
import { buildSettingsHostSectionRoute } from "@/utils/host-routes";

const MOCK_PROVIDER_LABEL = "Mock Load Test";

test.describe("Agent profiles settings", () => {
  test("legacy model favourites migrate into provider-and-model-only host profiles", async ({
    page,
  }) => {
    const seed = await seedAgentProfiles([]);
    const migratedProfile = {
      id: "legacy_favorite:mock:one-minute-stream",
      name: "One minute stream",
      provider: "mock",
      model: "one-minute-stream",
    };

    try {
      await stageLegacyFavoritesForHostMigration(page, [
        { provider: "mock", modelId: "one-minute-stream" },
      ]);

      await expectHostAgentProfiles([migratedProfile]);
      await openAgentProfileSettings(page);
      await expectAgentProfile(page, {
        name: migratedProfile.name,
        tags: [MOCK_PROVIDER_LABEL, "One minute stream"],
      });
    } finally {
      await seed.restore();
    }
  });

  test("host owner creates, edits, reorders and removes agent profiles", async ({ page }) => {
    // The journey owns the list, so it starts from an empty one whatever the
    // worker ran before, and hands it back untouched.
    const seed = await seedAgentProfiles([]);

    try {
      await test.step("the section starts empty", async () => {
        await openAgentProfileSettings(page);
        await expectNoAgentProfiles(page);
      });

      await test.step("create a profile from the modal", async () => {
        await createAgentProfileFromEmptyState(page, {
          name: "UI work",
          provider: MOCK_PROVIDER_LABEL,
          model: "Ten second stream",
          thinking: "High",
          mode: "Approval test",
          notes: "Use for UI work — components, layout and design tokens.",
        });
        await expectAgentProfile(page, {
          name: "UI work",
          tags: [MOCK_PROVIDER_LABEL, "Ten second stream", "Approval test", "High"],
          notes: "Use for UI work — components, layout and design tokens.",
        });
      });

      await test.step("editing to a model without thinking levels drops the level", async () => {
        await editAgentProfile(page, "UI work", {
          model: "One minute stream",
          notes: "Now for quick checks.",
        });
        await expectAgentProfile(page, {
          name: "UI work",
          tags: [MOCK_PROVIDER_LABEL, "One minute stream", "Approval test"],
          notes: "Now for quick checks.",
        });
        await expectAgentProfileTagsGone(page, {
          name: "UI work",
          tags: ["Ten second stream", "High"],
        });
      });

      await test.step("the edit form reopens on what was stored", async () => {
        await expectAgentProfileForm(page, "UI work", {
          provider: MOCK_PROVIDER_LABEL,
          model: "One minute stream",
          mode: "Approval test",
          notes: "Now for quick checks.",
        });
      });

      await test.step("a second profile lands below the first", async () => {
        await createAgentProfile(page, {
          name: "Deep review",
          provider: MOCK_PROVIDER_LABEL,
          model: "Five minute stream",
        });
        await expectAgentProfileOrder(page, ["UI work", "Deep review"]);
      });

      await test.step("reorder puts the second profile on top", async () => {
        await moveAgentProfileUp(page, "Deep review");
        await expectAgentProfileOrder(page, ["Deep review", "UI work"]);
      });

      await test.step("removing a profile confirms by name", async () => {
        await removeAgentProfile(page, "UI work");
        await expectAgentProfileOrder(page, ["Deep review"]);
      });

      await test.step("removing the last profile returns the empty state", async () => {
        await removeAgentProfile(page, "Deep review");
        await expectNoAgentProfiles(page);
      });
    } finally {
      await seed.restore();
    }
  });

  test("an unknown saved model stays visible, unresolved, and intact through a save", async ({
    page,
  }) => {
    const stored = {
      id: "agent_profile_e2e_retired",
      name: "Retired model work",
      provider: "codex",
      model: "retired-model-e2e",
    };
    const seed = await seedAgentProfiles([stored]);

    try {
      await openAgentProfileSettings(page);
      const row = agentProfileRow(page, stored.name);
      await expect(row.getByTestId("agent-profile-tag-unresolved-model")).toHaveText(
        "retired-model-e2e",
      );
      await expect(row.getByTestId(`agent-profile-unresolved-${stored.id}`)).toBeVisible();

      await row.getByRole("button", { name: "Edit profile", exact: true }).click();
      const form = page.getByTestId("agent-profile-edit-page");
      await expect(form).toBeVisible({ timeout: 30_000 });
      await expect(page.getByTestId("agent-profile-model-trigger")).toHaveAttribute(
        "aria-label",
        "Model (retired-model-e2e)",
      );
      await expect(page.getByTestId("agent-profile-model-field")).toContainText(
        "Not offered by this host",
      );

      // Saving without touching the model keeps the stored id exactly.
      await form.getByRole("button", { name: "Save", exact: true }).click();
      await expectAppRoute(page, buildSettingsHostSectionRoute(getServerId(), "agents"), {
        timeout: 30_000,
      });
      await expectHostAgentProfiles([stored]);
    } finally {
      await seed.restore();
    }
  });

  test("editing keeps fields a newer client stored and leaves omitted fields unspecified", async ({
    page,
  }) => {
    const stored = {
      id: "agent_profile_e2e_future",
      name: "Future proof",
      provider: "codex",
      model: "offline-model",
      futureSetting: { nested: ["keep", 42] },
    };
    const seed = await seedAgentProfiles([stored]);

    try {
      await openAgentProfileSettings(page);
      await agentProfileRow(page, stored.name)
        .getByRole("button", { name: "Edit profile", exact: true })
        .click();
      const form = page.getByTestId("agent-profile-edit-page");
      await expect(form).toBeVisible({ timeout: 30_000 });

      // Clear the model through the explicit blank row, then save.
      await page.getByTestId("agent-profile-model-trigger").click();
      await page.getByTestId("agent-profile-leave-unchanged-option").click();
      await form.getByRole("button", { name: "Save", exact: true }).click();
      await expectAppRoute(page, buildSettingsHostSectionRoute(getServerId(), "agents"), {
        timeout: 30_000,
      });

      await expectHostAgentProfiles([
        {
          id: stored.id,
          name: stored.name,
          provider: stored.provider,
          futureSetting: { nested: ["keep", 42] },
        },
      ]);
    } finally {
      await seed.restore();
    }
  });

  test.describe("compact layout", () => {
    test.use({ viewport: { width: 390, height: 844 } });

    test("nested create and edit navigate through the compact settings stack", async ({ page }) => {
      const seed = await seedAgentProfiles([]);

      try {
        await openAgentProfileSettings(page);
        await expectNoAgentProfiles(page);

        await page
          .getByTestId("agent-profiles-card")
          .getByRole("button", { name: "New profile", exact: true })
          .click();
        const form = page.getByTestId("agent-profile-edit-page");
        await expect(form).toBeVisible({ timeout: 30_000 });
        await form.getByRole("textbox", { name: "Name", exact: true }).fill("Compact work");
        await form.getByTestId("agent-profile-provider-trigger").click();
        // At compact width the combobox is a bottom sheet, so there is no
        // desktop container: target the option row itself, which both surfaces render.
        await page.getByTestId("agent-profile-provider-option-codex").click();
        await form.getByRole("button", { name: "Save", exact: true }).click();
        await expectAppRoute(page, buildSettingsHostSectionRoute(getServerId(), "agents"), {
          timeout: 30_000,
        });
        await expectAgentProfile(page, { name: "Compact work", tags: ["codex"] });

        // Reopen through the row, then leave with the header Back button.
        await agentProfileRow(page, "Compact work")
          .getByRole("button", { name: "Edit profile", exact: true })
          .click();
        await expect(form).toBeVisible({ timeout: 30_000 });
        await page.getByRole("button", { name: "Back", exact: true }).click();
        await expectAppRoute(page, buildSettingsHostSectionRoute(getServerId(), "agents"), {
          timeout: 30_000,
        });
        await expect(page.getByTestId("agent-profiles-card")).toBeVisible({ timeout: 30_000 });
      } finally {
        await seed.restore();
      }
    });
  });
});

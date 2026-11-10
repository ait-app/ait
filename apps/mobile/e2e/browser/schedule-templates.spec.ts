import { test, expect } from "../support/fixtures";
import { seedWorkspace } from "../support/helpers/seed-client";
import { gotoWorkspace } from "../support/helpers/launcher";

test.use({
  locale: "zh-CN",
  timezoneId: "Asia/Shanghai",
  viewport: { width: 1440, height: 1000 },
  colorScheme: "dark",
  video: "off",
});

test("browses template cards, personalizes a task and preserves it across editor sections", async ({
  page,
}, testInfo) => {
  const workspace = await seedWorkspace({ repoPrefix: "ait-templates-", title: "定时任务" });
  try {
    await gotoWorkspace(page, workspace.workspaceId);
    await page.getByTestId("workspace-new-tab-button").filter({ visible: true }).first().click();
    await page.getByTestId("workspace-new-tab-menu-schedule").filter({ visible: true }).click();
    await page.getByTestId("workspace-schedules-new").click();
    const gallery = page.getByTestId("schedule-template-gallery");
    await expect(gallery).toBeVisible();
    await expect(page.getByTestId(/^schedule-template-option-/)).toHaveCount(5);
    await expect(page.getByTestId("schedule-form-submit")).toHaveCount(0);
    const first = await page
      .getByTestId("schedule-template-option-nightly-test-heal")
      .boundingBox();
    const second = await page.getByTestId("schedule-template-option-pr-gatekeeper").boundingBox();
    expect(first && second && Math.abs(first.y - second.y) < 2 && second.x > first.x).toBe(true);
    await page.screenshot({ path: testInfo.outputPath("templates-gallery-desktop.png") });

    await page.getByTestId("schedule-template-option-domain-radar").click();
    await expect(page.getByTestId("schedule-template-apply")).toBeDisabled();
    await page.getByTestId("schedule-template-param-topic").fill("AI Agent 与 Rust 生态");
    await page.screenshot({ path: testInfo.outputPath("template-personalize-desktop.png") });
    await page.getByTestId("schedule-template-apply").click();
    await expect(page.getByTestId("schedule-name-input")).toHaveValue("主题简报");
    await expect(page.getByTestId("schedule-prompt-input")).toHaveValue(/AI Agent 与 Rust 生态/);
    await expect(page.getByTestId("cadence-cron-expression")).toHaveValue("0 9 * * 1-5");
    await expect(page.getByTestId("schedule-template-param-topic")).toHaveCount(0);
    await page.screenshot({ path: testInfo.outputPath("template-editor-desktop.png") });

    await page.getByTestId("schedule-section-execution").click();
    await expect(page.getByTestId("schedule-max-runs-input")).toHaveValue("");
    await page.getByTestId("schedule-max-runs-input").fill("3");
    await page.getByTestId("schedule-section-content").click();
    await expect(page.getByTestId("schedule-prompt-input")).toHaveValue(/AI Agent 与 Rust 生态/);
    await page.getByTestId("schedule-template-browse").click();
    await page.getByTestId("schedule-template-blank").click();
    await page.getByTestId("schedule-section-execution").click();
    await expect(page.getByTestId("schedule-max-runs-input")).toHaveValue("3");
    await page.getByTestId("schedule-section-content").click();
    await page.getByTestId("schedule-cadence-kind-trigger").click();
    await page.getByTestId("schedule-cadence-kind-every").click();
    await page.getByTestId("cadence-interval-amount").fill("2.5");
    await page.getByTestId("schedule-template-browse").click();
    await page.getByTestId("schedule-template-blank").click();
    await expect(page.getByTestId("cadence-interval-amount")).toHaveValue("2.5");
    await expect(page.getByTestId("schedule-form-submit")).toBeDisabled();
    await page.getByTestId("cadence-interval-amount").fill("2");
    await expect(page.getByTestId("schedule-form-submit")).toBeEnabled();
    await page.getByTestId("schedule-template-browse").click();
    await page.setViewportSize({ width: 390, height: 844 });
    await expect(gallery).toBeVisible();
    await expect
      .poll(async () => {
        const a = await page
          .getByTestId("schedule-template-option-nightly-test-heal")
          .boundingBox();
        const b = await page.getByTestId("schedule-template-option-pr-gatekeeper").boundingBox();
        return Boolean(a && b && Math.abs(a.x - b.x) < 2 && b.y > a.y);
      })
      .toBe(true);
    await page.screenshot({ path: testInfo.outputPath("templates-gallery-mobile.png") });
  } finally {
    await workspace.cleanup();
  }
});

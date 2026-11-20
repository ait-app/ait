import { buildHostWorkspaceRoute } from "@/utils/host-routes";
import { expect, test } from "../support/fixtures";
import { expectComposerEditable, submitMessage } from "../support/helpers/composer";
import { getServerId } from "../support/helpers/server-id";

const program = process.env.AIT_TEST_MINIMAX_CODE_BIN;
test.skip(!program, "requires an explicitly selected, authenticated mcode executable");
test.use({ e2eDaemonEnvironment: { AIT_SERVER_MINIMAX_CODE_BIN: program ?? "missing-mcode" } });

test("MiniMax Code ask_user opens one question per step and accepts the answer", async ({
  page,
  withWorkspace,
  e2eWorkerClient,
}) => {
  test.setTimeout(240_000);
  const workspace = await withWorkspace({ prefix: "minimax-question-" });
  const agent = await e2eWorkerClient.createAgent({
    provider: "minimax-code",
    cwd: workspace.repoPath,
    workspaceId: workspace.workspaceId,
    title: "MiniMax Code question validation",
  });
  const route = buildHostWorkspaceRoute(getServerId(), workspace.workspaceId);
  await page.goto(`${route}?open=${encodeURIComponent(`agent:${agent.id}`)}`);
  await expectComposerEditable(page);
  await submitMessage(
    page,
    "Call your ask_user tool now, in one call with exactly two single-choice steps: the first with header Language asking whether I prefer Rust or Go, the second with header Storage asking whether I prefer Postgres or SQLite. Give every option a description. After I answer, reply with exactly the two options I chose.",
  );

  const card = page.getByTestId("question-form-card");
  await expect(card).toBeVisible({ timeout: 90_000 });
  const tabs = card.getByRole("tab");
  await expect(tabs).toHaveText(["Language", "Storage"]);
  if (process.env.AIT_E2E_SCREENSHOT_DIR) {
    await page.screenshot({ path: `${process.env.AIT_E2E_SCREENSHOT_DIR}/minimax-question.png` });
  }
  await card.getByRole("radio", { name: "Rust" }).click();
  await card.getByRole("radio", { name: "SQLite" }).click();
  await card.getByTestId("question-form-primary-action").click();
  await expect(card).toBeHidden({ timeout: 30_000 });

  const finished = await e2eWorkerClient.waitForFinish(agent.id, 120_000);
  expect(finished.status).toBe("idle");
  expect(finished.final?.lastError).toBeFalsy();
  const reply = page.getByTestId("assistant-message").last();
  await expect(reply).toContainText("Rust");
  await expect(reply).toContainText("SQLite");
});

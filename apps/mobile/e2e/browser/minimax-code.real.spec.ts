import { buildHostWorkspaceRoute } from "@/utils/host-routes";
import { expect, test } from "../support/fixtures";
import { expectComposerEditable, submitMessage } from "../support/helpers/composer";
import { getServerId } from "../support/helpers/server-id";

const program = process.env.AIT_TEST_MINIMAX_CODE_BIN;
test.skip(!program, "requires an explicitly selected, authenticated mcode executable");
test.use({ e2eDaemonEnvironment: { AIT_SERVER_MINIMAX_CODE_BIN: program ?? "missing-mcode" } });

test("MiniMax Code streams a response and retains conversation after browser reload", async ({
  page,
  withWorkspace,
  e2eWorkerClient,
}, testInfo) => {
  test.setTimeout(180_000);
  const workspace = await withWorkspace({ prefix: "minimax-ui-" });
  const agent = await e2eWorkerClient.createAgent({
    provider: "minimax-code",
    cwd: workspace.repoPath,
    workspaceId: workspace.workspaceId,
    title: "MiniMax Code UI validation",
  });
  const route = buildHostWorkspaceRoute(getServerId(), workspace.workspaceId);
  await page.goto(`${route}?open=${encodeURIComponent(`agent:${agent.id}`)}`);
  await expectComposerEditable(page);
  await submitMessage(
    page,
    "Remember AIT_MINIMAX_UI_7391 for the next message. Reply exactly AIT_MINIMAX_UI_7391. Do not use tools.",
  );
  const finished = await e2eWorkerClient.waitForFinish(agent.id, 120_000);
  expect(finished.status).toBe("idle");
  expect(finished.final?.lastError).toBeFalsy();
  await expect(page.getByTestId("assistant-message").last()).toContainText("AIT_MINIMAX_UI_7391");

  await page.reload();
  await expectComposerEditable(page);
  await submitMessage(page, "What code did I ask you to remember? Reply only with the code.");
  const recalled = await e2eWorkerClient.waitForFinish(agent.id, 120_000);
  expect(recalled.status).toBe("idle");
  expect(recalled.final?.lastError).toBeFalsy();
  await expect(page.getByTestId("assistant-message").last()).toContainText("AIT_MINIMAX_UI_7391");
  await testInfo.attach("MiniMax Code conversation", {
    body: await page.screenshot(),
    contentType: "image/png",
  });
});

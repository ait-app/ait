import { expect, test, type Page } from "../support/fixtures";
import {
  composerLocator,
  expectComposerDraft,
  fillComposerDraft,
} from "../support/helpers/composer";
import { openAgentRoute } from "../support/helpers/mock-agent";
import { seedWorkspace } from "../support/helpers/seed-client";
import { daemonWsRoutePattern } from "../support/helpers/daemon-port";

interface RewindFrame {
  type?: string;
  method?: string;
  request_id?: string;
  params?: { agentId?: string };
}

async function holdRewindCompletion(page: Page, agentId: string) {
  let requestId: string | undefined;
  let released = false;
  let responseReceived = false;
  const forwards: Array<() => void> = [];
  await page.routeWebSocket(daemonWsRoutePattern(), (socket) => {
    const server = socket.connectToServer();
    socket.onMessage((message) => {
      if (typeof message === "string") {
        const frame = JSON.parse(message) as RewindFrame;
        if (
          frame.type === "request" &&
          frame.method === "agent.rewind.request" &&
          frame.params?.agentId === agentId
        ) {
          requestId = frame.request_id;
        }
      }
      server.send(message);
    });
    server.onMessage((message) => {
      if (!released && typeof message === "string") {
        const frame = JSON.parse(message) as RewindFrame;
        const isResponse =
          (frame.type === "response" || frame.type === "error") &&
          requestId !== undefined &&
          frame.request_id === requestId;
        const isReplacement =
          frame.type === "event" &&
          frame.method === "agent.timeline.replacement" &&
          frame.params?.agentId === agentId;
        if (isResponse || isReplacement) {
          forwards.push(() => socket.send(message));
          if (isResponse) responseReceived = true;
          return;
        }
      }
      socket.send(message);
    });
  });
  return {
    waitForDelayedResponse: () => expect.poll(() => responseReceived).toBe(true),
    release() {
      released = true;
      for (const forward of forwards.splice(0)) forward();
    },
  };
}

test("keeps the composer usable while Codex conversation rewind is pending", async ({ page }) => {
  const workspace = await seedWorkspace({ repoPrefix: "ait-rewind-responsive-" });
  const retainedPrompt = "Keep this completed turn.";
  const rewoundPrompt = "Rewind this completed turn.";
  const draft = "A draft typed while rewind is pending.";

  try {
    const agent = await workspace.client.createAgent({
      provider: "codex",
      model: "offline-model",
      cwd: workspace.repoPath,
      workspaceId: workspace.workspaceId,
      title: "Rewind responsiveness",
    });
    for (const prompt of [retainedPrompt, rewoundPrompt]) {
      await workspace.client.sendAgentMessage(agent.id, prompt);
      expect((await workspace.client.waitForFinish(agent.id, 15_000)).status).toBe("idle");
    }

    const completion = await holdRewindCompletion(page, agent.id);
    try {
      await openAgentRoute(page, { workspaceId: workspace.workspaceId, agentId: agent.id });
      const userMessage = page.getByTestId("user-message").filter({ hasText: rewoundPrompt });
      await expect(userMessage).toBeVisible();
      await userMessage.getByText(rewoundPrompt, { exact: true }).hover();
      await userMessage.getByTestId("rewind-menu-trigger").click();
      await page.getByTestId("rewind-menu-conversation").click();
      await completion.waitForDelayedResponse();

      await expect(page.getByTestId("rewind-menu-content")).toHaveCount(0);
      await expect(userMessage.getByTestId("rewind-menu-trigger")).toBeDisabled();
      await composerLocator(page).click();
      await fillComposerDraft(page, draft);
      await expectComposerDraft(page, draft);

      completion.release();
      await expect(userMessage).toHaveCount(0);
      await expect(
        page.getByTestId("user-message").filter({ hasText: retainedPrompt }),
      ).toBeVisible();
      await expectComposerDraft(page, draft);
    } finally {
      completion.release();
    }
  } finally {
    await workspace.cleanup();
  }
});

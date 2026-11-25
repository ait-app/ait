import { mkdir } from "node:fs/promises";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { expect, test } from "../support/fixtures";
import { gotoWorkspace } from "../support/helpers/launcher";
import { seedWorkspace } from "../support/helpers/seed-client";

// Only platform data is simulated. Render the production app, Settings scroll container,
// switches and chat composer through Metro against the isolated Rust daemon.
test("chat reception stays in scrollable Settings on desktop and mobile", async ({
  page,
}, testInfo) => {
  test.setTimeout(180_000);
  const workspace = await seedWorkspace({ repoPrefix: "bot-reception-ui-" });
  const client = workspace.client as unknown as DaemonClient;
  const channels = [
    {
      id: "11111111-1111-4111-8111-111111111111",
      config: {
        kind: "feishu",
        appId: "cli_ops",
        secretRef: "env:AIT_SERVER_CREDENTIAL_E2E",
        allowedSenders: [],
        allowGroups: false,
      },
      enabled: true,
      cursor: null,
      deleted: false,
    },
    {
      id: "22222222-2222-4222-8222-222222222222",
      config: {
        kind: "lark",
        appId: "cli_support",
        secretRef: "env:AIT_SERVER_CREDENTIAL_E2E",
        allowedSenders: [],
        allowGroups: false,
      },
      enabled: true,
      cursor: null,
      deleted: false,
    },
  ];
  const chats = Array.from({ length: 6 }, (_, index) => ({
    channelId: channels[index % 2]!.id,
    peer: `oc_test_${index + 1}`,
    group: index !== 5,
    enabled: true,
    listenAllMessages: index === 0,
  }));
  await page.routeWebSocket(/\/v1\/ws(?:\?|$)/, (ws) => {
    const server = ws.connectToServer();
    ws.onMessage((data) => {
      if (typeof data !== "string") {
        server.send(data);
        return;
      }
      const message = JSON.parse(data);
      let result: unknown;
      if (message.method === "bot.chat.list.request") result = { chats };
      else if (message.method === "bot.channel.list.request") result = { channels };
      else if (message.method === "bot.channel.status.request")
        result = {
          channelId: message.params.channelId,
          enabled: true,
          connection: { state: "connected" },
        };
      else if (message.method === "bot.chat.update.request") {
        const chat = chats.find(
          (item) =>
            item.channelId === message.params.channelId && item.peer === message.params.peer,
        )!;
        Object.assign(chat, {
          enabled: message.params.enabled,
          listenAllMessages: message.params.listenAllMessages,
        });
        result = { chat };
      }
      if (result)
        ws.send(JSON.stringify({ type: "response", request_id: message.request_id, result }));
      else server.send(data);
    });
  });
  try {
    await client.botRequest("bot.setup.request", {
      workspaceId: workspace.workspaceId,
      execution: { provider: "codex", model: "offline-model", modeId: null, thinking: null },
    });
    await page.goto("/settings/about");
    await page.getByTestId("settings-experimental-features").click();
    await gotoWorkspace(page, workspace.workspaceId);
    await page.getByTestId("workspace-new-tab-button").filter({ visible: true }).first().click();
    await page.getByTestId("workspace-new-tab-menu-bot").filter({ visible: true }).click();
    // Metro compiles the Bot tab lazily on the first visit. Wait for its mounted surface.
    await expect(page.getByTestId("workspace-bot-tab")).toBeVisible({ timeout: 60_000 });
    const chat = page.getByTestId("bot-chat");
    await expect(chat).toBeVisible();
    await expect(page.getByTestId("bot-channels")).toHaveCount(0);
    const output = process.env.AIT_REVIEW_SCREENSHOTS ?? testInfo.outputPath("screenshots");
    await mkdir(output, { recursive: true });
    for (const viewport of [
      { name: "desktop", width: 1440, height: 1000 },
      { name: "mobile", width: 390, height: 844 },
    ]) {
      await page.setViewportSize(viewport);
      await expect(chat.getByRole("textbox")).toBeVisible();
      expect((await chat.boundingBox())!.height).toBeGreaterThan(viewport.height / 2);
      await page.screenshot({ path: `${output}/${viewport.name}-chat.png` });
      await page.getByTestId("bot-show-settings").click();
      const accounts = page.getByTestId("bot-channels");
      await expect(
        accounts.locator('[data-testid^="bot-chat-policy-"][data-testid$="-trigger"]'),
      ).toHaveCount(6);
      await accounts
        .getByText("Channels", { exact: true })
        .evaluate((element) => element.scrollIntoView({ block: "start" }));
      await expect(accounts.getByText("Feishu · cli_ops", { exact: true })).toBeVisible();
      await expect(accounts.getByText("Lark · cli_support", { exact: true })).toHaveCount(1);
      await expect(
        accounts.getByText("Each chat has its own reception settings.", { exact: false }),
      ).toHaveCount(0);
      expect(
        await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
      ).toBe(true);
      // Each account has one card, and none of its conversation rows has another border box.
      for (const channel of channels) {
        const card = page.getByTestId(`bot-channel-${channel.id}-card`);
        expect(
          await card.evaluate(
            (element) =>
              [...element.querySelectorAll("div")].filter((child) => {
                const style = getComputedStyle(child);
                return (
                  !child.closest('[role="button"], [role="switch"]') &&
                  parseFloat(style.borderLeftWidth) > 0 &&
                  parseFloat(style.borderRightWidth) > 0
                );
              }).length,
          ),
        ).toBe(0);
      }
      await page.screenshot({ path: `${output}/${viewport.name}-settings.png` });
      const mode = page.getByTestId(`bot-chat-policy-${channels[1]!.id}-oc_test_2-trigger`);
      await mode.scrollIntoViewIfNeeded();
      if ((await mode.textContent()) !== "Off") {
        await mode.click();
        await page.getByRole("menuitem", { name: "Off", exact: true }).click();
      }
      await expect(mode).toContainText("Off");
      const privateMode = page.getByTestId(`bot-chat-policy-${channels[1]!.id}-oc_test_6-trigger`);
      await privateMode.scrollIntoViewIfNeeded();
      await expect(privateMode).toBeInViewport();
      await privateMode.click();
      await expect(page.getByRole("menuitem")).toHaveCount(2);
      await page.getByRole("menuitem", { name: "On", exact: true }).click();
      await page.getByTestId("bot-show-chat").click();
      await expect(chat).toBeVisible();
      await expect(accounts).toHaveCount(0);
    }
  } finally {
    await workspace.cleanup();
  }
});

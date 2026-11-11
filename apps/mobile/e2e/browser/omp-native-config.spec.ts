import { AgentConfigRejectedError } from "@ait/client/internal/daemon-client";
import { expect, test } from "../support/fixtures";
import { connectDaemonClient } from "../support/helpers/daemon-client-loader";
import { openAgentRoute } from "../support/helpers/mock-agent";
import { seedWorkspace } from "../support/helpers/seed-client";

// Browser consumer path for native-confirmed OMP configuration (PR612 task05):
// a real daemon drives the offline OMP RPC fixture through the same agent-controls
// UI a user touches. Acknowledged selections settle and persist; a config change
// refused while the session is busy keeps the old value and surfaces a localized
// summary instead of a raw host error.
test.describe("OMP native-confirmed configuration", () => {
  test.describe.configure({ timeout: 180_000 });

  test("acknowledged selections persist and busy rejections keep the old value", async ({
    page,
  }) => {
    const workspace = await seedWorkspace({ repoPrefix: "omp-native-config-" });
    try {
      const agent = await workspace.client.createAgent({
        provider: "omp",
        cwd: workspace.repoPath,
        workspaceId: workspace.workspaceId,
        title: "OMP native config",
      });
      await openAgentRoute(page, {
        workspaceId: workspace.workspaceId,
        agentId: agent.id,
      });

      const thinkingSelector = page
        .getByTestId("agent-thinking-selector")
        .filter({ visible: true });
      await expect(thinkingSelector).toBeVisible({ timeout: 60_000 });

      // Acknowledged thinking selection: the native session confirms "high" for the
      // default fixture model and the trigger reflects the confirmed value.
      await thinkingSelector.click();
      await page.getByRole("button", { name: "High", exact: true }).click();
      await expect(thinkingSelector).toHaveAccessibleName(/High/, { timeout: 30_000 });

      // Start a turn that stays active, then attempt a config change through the same
      // SDK the UI uses: the native session is busy, the request is refused, and the
      // stored selection keeps its prior confirmed value.
      await workspace.client.sendAgentMessage(agent.id, "wait");
      const sdk = await connectDaemonClient<{
        connect(): Promise<void>;
        close(): Promise<void>;
        setAgentThinkingOption(agentId: string, thinkingOptionId: string | null): Promise<unknown>;
      }>({ clientIdPrefix: "omp-config-busy" });
      try {
        await expect(sdk.setAgentThinkingOption(agent.id, "low")).rejects.toBeInstanceOf(
          AgentConfigRejectedError,
        );
      } finally {
        await sdk.close().catch(() => undefined);
      }
      // The rejected change keeps the prior confirmed value, not an optimistic one.
      await expect(thinkingSelector).toHaveAccessibleName(/High/);

      // The same busy rejection through the UI shows the localized summary toast.
      await thinkingSelector.click();
      await page.getByRole("button", { name: "Low", exact: true }).click();
      await expect(page.getByTestId("app-toast-message")).toContainText(
        "Configuration change was rejected",
        { timeout: 30_000 },
      );

      // Settle the turn so cleanup leaves no running writer.
      await workspace.client.cancelAgent(agent.id);
      await workspace.client.waitForFinish(agent.id, 30_000);
    } finally {
      await workspace.cleanup();
    }
  });
});

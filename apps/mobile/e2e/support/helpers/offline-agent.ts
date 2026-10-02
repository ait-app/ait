import { seedWorkspace } from "./seed-client";
import type { MockAgentOptions, MockAgentWorkspace } from "./mock-agent";

/** Use the worker's offline stdio Codex peer, never an authenticated model CLI. */
export async function seedOfflineAgentWorkspace(
  options: MockAgentOptions,
): Promise<MockAgentWorkspace> {
  const workspace = await seedWorkspace({
    repoPrefix: options.repoPrefix,
    repo: options.repo,
    port: options.port,
  });
  try {
    const agent = await workspace.client.createAgent({
      provider: "codex",
      model: "offline-model",
      modeId: "auto",
      cwd: workspace.repoPath,
      workspaceId: workspace.workspaceId,
      title: options.title,
      ...(options.thinkingOptionId ? { thinkingOptionId: options.thinkingOptionId } : {}),
    });
    if (options.initialPrompt) {
      await workspace.client.sendAgentMessage(agent.id, options.initialPrompt);
      await workspace.client.waitForFinish(agent.id, 15000);
    }
    return {
      agentId: agent.id,
      workspaceId: workspace.workspaceId,
      cwd: workspace.repoPath,
      client: workspace.client,
      cleanup: workspace.cleanup,
    };
  } catch (error) {
    await workspace.cleanup();
    throw error;
  }
}

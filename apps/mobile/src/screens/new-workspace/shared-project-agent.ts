import type {
  CreateAgentRequestOptions,
  CreateWorkspaceRequestOptions,
} from "@ait/client/internal/daemon-client";
import type { AgentSnapshotPayload } from "@ait/protocol/messages";

/** A shared-project guest must create the workspace before placing an Agent in it. */
export async function createSharedProjectAgent<
  Workspace extends { id: string; workspaceDirectory: string },
>(input: {
  createWorkspace: () => Promise<Workspace>;
  createAgent: (request: CreateAgentRequestOptions) => Promise<AgentSnapshotPayload>;
  agent: NonNullable<CreateWorkspaceRequestOptions["agent"]> & {
    config: NonNullable<CreateAgentRequestOptions["config"]>;
  };
}): Promise<AgentSnapshotPayload> {
  const workspace = await input.createWorkspace();
  return input.createAgent({
    ...input.agent,
    idempotencyKey: input.agent.clientMessageId,
    workspaceId: workspace.id,
    config: { ...input.agent.config, cwd: workspace.workspaceDirectory },
  });
}

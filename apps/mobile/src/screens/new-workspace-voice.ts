import type { CreateWorkspaceRequestOptions } from "@ait/client/internal/daemon-client";
import type { AgentAttachment, AgentSnapshotPayload } from "@ait/protocol/messages";

/** Voice needs an empty Agent, whereas an empty text submission creates only a workspace. */
export async function createWorkspaceVoiceAgent<TWorkspace>(input: {
  config: NonNullable<NonNullable<CreateWorkspaceRequestOptions["agent"]>["config"]>;
  ensureWorkspace: (request: {
    cwd: string;
    prompt: string;
    attachments: AgentAttachment[];
    withInitialAgent: boolean;
    agent: CreateWorkspaceRequestOptions["agent"];
  }) => Promise<{ workspace: TWorkspace; agent?: AgentSnapshotPayload }>;
  onCreated: (workspace: TWorkspace, agent: AgentSnapshotPayload) => void;
  isCurrent: () => boolean;
  clearConsumedDraft: () => void;
}): Promise<string | null> {
  const { workspace, agent } = await input.ensureWorkspace({
    cwd: input.config.cwd,
    prompt: "",
    attachments: [],
    withInitialAgent: true,
    agent: { config: input.config },
  });
  if (!agent) throw new Error("Workspace creation returned no agent");
  input.clearConsumedDraft();
  // Leaving the creation screen must not activate the microphone in the background.
  if (!input.isCurrent()) return null;
  input.onCreated(workspace, agent);
  return agent.id;
}

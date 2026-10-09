import type { AgentSessionConfig } from "@ait/protocol/agent-types";
import type { AgentSnapshotPayload, CreateAgentRequestMessage } from "@ait/protocol/messages";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { encodeImages } from "@/utils/encode-images";
import type { UserMessageImageAttachment } from "@/types/stream";

export interface WorkspaceDraftAgentRequest {
  workspaceId: string;
  config: AgentSessionConfig;
  text: string;
  clientMessageId: string;
  images?: UserMessageImageAttachment[];
  attachments?: CreateAgentRequestMessage["attachments"];
}

/**
 * A submission owns its creation key. Replaying it preserves the key; submitting
 * again after failure gets a new message identity from the draft create flow.
 */
export async function requestWorkspaceDraftAgent(
  client: DaemonClient,
  request: WorkspaceDraftAgentRequest,
): Promise<AgentSnapshotPayload> {
  const images = await encodeImages(request.images);
  return await client.createAgent({
    idempotencyKey: request.clientMessageId,
    config: request.config,
    workspaceId: request.workspaceId,
    clientMessageId: request.clientMessageId,
    ...(request.text ? { initialPrompt: request.text } : {}),
    ...(images && images.length > 0 ? { images } : {}),
    ...(request.attachments && request.attachments.length > 0
      ? { attachments: request.attachments }
      : {}),
  });
}

import {
  buildDraftWorkspaceAttachmentScopeKey,
  useWorkspaceAttachmentsStore,
} from "@/attachments/workspace-attachments-store";
import { buildDraftAgentSetup } from "@/client-slash-commands";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { generateDraftId } from "@/stores/draft-keys";
import { navigateToWorkspace } from "@/stores/navigation-active-workspace-store";
import { useSessionStore } from "@/stores/session-store";
import { buildBtwTransferAttachment } from "./transfer";

/** Expand a BTW conversation into the same editable draft used by a regular chat fork. */
export function useTransferSideQuestion(input: {
  serverId: string;
  agentId: string;
  workspaceId?: string | null;
}): (recordId: string) => Promise<void> {
  const client = useHostRuntimeClient(input.serverId);
  return async (recordId) => {
    const agent = useSessionStore.getState().sessions[input.serverId]?.agents.get(input.agentId);
    if (!client || !agent || !input.workspaceId) {
      throw new Error("The source conversation is unavailable.");
    }
    const payload = await client.transferSideQuestion({ agentId: input.agentId, recordId });
    const draftId = generateDraftId();
    const attachment = buildBtwTransferAttachment({ ...input, draftId, ...payload });
    useWorkspaceAttachmentsStore.getState().setWorkspaceAttachments({
      scopeKey: buildDraftWorkspaceAttachmentScopeKey(draftId),
      attachments: [attachment],
    });
    navigateToWorkspace({
      serverId: input.serverId,
      workspaceId: input.workspaceId,
      target: {
        kind: "draft",
        draftId,
        setup: {
          ...buildDraftAgentSetup(agent),
          provider: payload.snapshot.provider,
          model: payload.snapshot.model,
        },
      },
    });
  };
}

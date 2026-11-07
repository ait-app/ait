import type { WorkspaceIsolation } from "@/create-agent-preferences/preferences";
import { pickerOptionId, type PickerItem } from "../new-workspace-picker-item";
import { createsWorktree } from "./isolation";

export interface WorkspaceCreationRequestIdentity {
  serverId: string;
  projectId: string | null;
  sourceDirectory: string | null;
  isolation: WorkspaceIsolation;
  selectedItem: PickerItem | null;
}

/**
 * A created workspace is reused across retries (e.g. when the first terminal or agent fails
 * after the workspace exists), but only while the user still targets the same host, project,
 * isolation and base. Any change there must create a new workspace.
 */
export function buildWorkspaceCreationRequestKey(input: WorkspaceCreationRequestIdentity): string {
  return JSON.stringify([
    input.serverId,
    input.projectId,
    input.sourceDirectory,
    input.isolation,
    createsWorktree(input.isolation) && input.selectedItem
      ? pickerOptionId(input.selectedItem)
      : null,
  ]);
}

/**
 * Only a request that creates no agent alongside the workspace may reuse one. The cached result
 * carries no agent (a failed terminal launch) or a previous request's agent, so an agent request
 * served from it would fail with "returned no agent" or resolve to the wrong agent.
 */
export function canReuseCreatedWorkspace(input: {
  cachedRequestKey: string | null;
  requestKey: string;
  createsAgent: boolean;
}): boolean {
  return !input.createsAgent && input.cachedRequestKey === input.requestKey;
}

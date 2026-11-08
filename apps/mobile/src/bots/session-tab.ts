import { collectAllTabs } from "@/stores/workspace-layout-actions";
import { useWorkspaceLayoutStore } from "@/stores/workspace-layout-store";
import { buildWorkspaceTabPersistenceKey } from "@/workspace-tabs/model";

/** Closes the tab of a Bot session that no longer exists, so it cannot strand a load error. */
export function closeBotSessionTab(serverId: string, workspaceId: string, agentId: string): void {
  const workspaceKey = buildWorkspaceTabPersistenceKey({ serverId, workspaceId });
  if (!workspaceKey) return;
  const store = useWorkspaceLayoutStore.getState();
  const layout = store.layoutByWorkspace[workspaceKey];
  if (!layout) return;
  // Hide as well as unpin: a fetched session looks like a root Agent, and reconcile reopens those.
  store.unpinAgent(workspaceKey, agentId);
  store.hideAgent(workspaceKey, agentId);
  const tab = collectAllTabs(layout.root).find(
    ({ target }) => target.kind === "agent" && target.agentId === agentId,
  );
  if (tab) store.closeTab(workspaceKey, tab.tabId);
}

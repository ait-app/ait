export const PARENT_AGENT_ID_LABEL = "paseo.parent-agent-id";
/** Written by a daemon that was called from another host: that host's `serverId` and the caller agent. */
export const PARENT_SERVER_ID_LABEL = "ait.parent.server-id";
export const REMOTE_PARENT_AGENT_ID_LABEL = "ait.parent.agent-id";
const OPEN_AGENT_TAB_LABEL_PREFIX = "paseo.open-agent-tab.";

export function getOpenAgentTabLabel(clientId: string): string {
  return `${OPEN_AGENT_TAB_LABEL_PREFIX}${clientId}`;
}

export function isOpenAgentTabLabel(label: string): boolean {
  return label.startsWith(OPEN_AGENT_TAB_LABEL_PREFIX);
}

export function getParentAgentIdFromLabels(labels: Record<string, unknown> | null | undefined) {
  const parentAgentId = labels?.[PARENT_AGENT_ID_LABEL];
  return typeof parentAgentId === "string" && parentAgentId.trim().length > 0
    ? parentAgentId.trim()
    : null;
}

function readNonEmptyLabel(labels: Record<string, unknown> | null | undefined, key: string) {
  const value = labels?.[key];
  return typeof value === "string" && value.trim().length > 0 ? value.trim() : null;
}

/** The calling agent on another host, only when both labels are present. */
export function getRemoteParentFromLabels(
  labels: Record<string, unknown> | null | undefined,
): { serverId: string; agentId: string } | null {
  const serverId = readNonEmptyLabel(labels, PARENT_SERVER_ID_LABEL);
  const agentId = readNonEmptyLabel(labels, REMOTE_PARENT_AGENT_ID_LABEL);
  return serverId && agentId ? { serverId, agentId } : null;
}

import type {
  HubAccountConnection,
  HubAccountEvent,
  HubDirectoryItem,
  HubDirectoryPage,
} from "@ait/client/internal/hub";

/** Internal notification metadata; preserve Host identity when summaries share an Agent ID. */
export async function loadHubAgentSummaries(
  connection: HubAccountConnection,
): Promise<HubDirectoryItem[]> {
  const items: HubDirectoryItem[] = [];
  let after: HubDirectoryPage["nextCursor"] = null;
  const seen = new Set<string>();
  do {
    const page: HubDirectoryPage = await connection.call("agent.list.request", {
      after,
      limit: 100,
    });
    items.push(...page.items);
    after = page.nextCursor;
    if (after) {
      const key = JSON.stringify(after);
      if (seen.has(key)) throw new Error("Agent summary pagination did not advance.");
      seen.add(key);
    }
  } while (after);
  return items;
}

export function hubAttentionNotification(
  event: HubAccountEvent,
  serverId: string,
  agents: readonly HubDirectoryItem[],
) {
  if (!event.params || typeof event.params !== "object" || Array.isArray(event.params)) return null;
  const params = event.params as Record<string, unknown>;
  if (event.method === "agent_attention_required") {
    const agent = agents.find((item) => item.hostId === event.hostId && item.id === params.agentId);
    const workspaceId = typeof agent?.data.workspaceId === "string" ? agent.data.workspaceId : null;
    if (typeof params.agentId !== "string" || typeof params.timestamp !== "string") return null;
    // An older directory snapshot may still say false when a new attention arrives first.
    const eventAt = Date.parse(params.timestamp);
    const summaryAt =
      typeof agent?.data.updatedAt === "string" ? Date.parse(agent.data.updatedAt) : NaN;
    const attentionAt =
      typeof agent?.data.attentionTimestamp === "string"
        ? Date.parse(agent.data.attentionTimestamp)
        : NaN;
    if (
      agent &&
      ((agent.data.requiresAttention === false &&
        (summaryAt >= eventAt || attentionAt === eventAt)) ||
        attentionAt > eventAt)
    )
      return null;
    const titles: Record<string, string> = {
      finished: "Agent finished",
      error: "Agent needs attention",
      permission: "Agent needs permission",
    };
    const title = typeof params.reason === "string" ? titles[params.reason] : undefined;
    if (!title) return null;
    return {
      title,
      body: typeof agent?.data.title === "string" ? agent.data.title : title,
      data: {
        serverId,
        hostId: event.hostId,
        eventId: event.eventId,
        attentionKey: JSON.stringify([serverId, "agent", params.agentId, params.timestamp]),
        agentId: params.agentId,
        ...(workspaceId ? { workspaceId } : {}),
      },
      key: JSON.stringify([serverId, "agent", params.agentId, params.timestamp]),
      agentId: params.agentId,
      terminalId: null,
    };
  }
  if (
    event.method === "terminal_attention_required" &&
    typeof params.terminalId === "string" &&
    typeof params.title === "string" &&
    typeof params.body === "string"
  ) {
    return {
      title: params.title,
      body: params.body,
      data: {
        serverId,
        hostId: event.hostId,
        eventId: event.eventId,
        attentionKey: JSON.stringify([
          serverId,
          "terminal",
          params.terminalId,
          params.title,
          params.body,
        ]),
        terminalId: params.terminalId,
        ...(typeof params.workspaceId === "string" ? { workspaceId: params.workspaceId } : {}),
      },
      key: JSON.stringify([serverId, "terminal", params.terminalId, params.title, params.body]),
      agentId: null,
      terminalId: params.terminalId,
    };
  }
  return null;
}

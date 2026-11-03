import type { DaemonClient } from "@ait/client/internal/daemon-client";
import type { AgentSnapshotPayload } from "@ait/protocol/messages";
import { useSessionStore } from "@/stores/session-store";
import { normalizeAgentSnapshot } from "@/utils/agent-snapshots";

type SubagentClient = Pick<DaemonClient, "listSubagents">;
const requests = new WeakMap<SubagentClient, Map<string, Promise<void>>>();

/** Keep discovery in the ordinary Agent replica so every surface observes the same identity. */
export async function acceptSubagentSnapshot(
  serverId: string,
  snapshot: AgentSnapshotPayload,
  client?: SubagentClient,
): Promise<void> {
  const { getHostRuntimeStore } = await import("@/runtime/host-runtime");
  const session = useSessionStore.getState().sessions[serverId];
  if (client && (session?.client !== client || !session.client.isConnected)) return;
  const previous = session?.agents.get(snapshot.id) ?? session?.agentDetails.get(snapshot.id);
  const agent = normalizeAgentSnapshot(snapshot, serverId);
  const parent = agent.parentAgentId
    ? (session?.agents.get(agent.parentAgentId) ?? session?.agentDetails.get(agent.parentAgentId))
    : undefined;
  getHostRuntimeStore().acceptAgentSnapshot(serverId, {
    ...agent,
    projectPlacement: previous?.projectPlacement ?? parent?.projectPlacement,
  });
}

/** Coalesce discovery while the shared directory subscription delivers subsequent changes. */
export function refreshUnifiedSubagents(
  client: SubagentClient,
  serverId: string,
  parentAgentId: string,
): Promise<void> {
  let pending = requests.get(client);
  if (!pending) {
    pending = new Map();
    requests.set(client, pending);
  }
  const key = `${serverId}\0${parentAgentId}`;
  const current = pending.get(key);
  if (current) return current;
  const request = client
    .listSubagents(parentAgentId)
    .then(async (payload) => {
      if (useSessionStore.getState().sessions[serverId]?.client !== client) return;
      for (const snapshot of payload.agents)
        await acceptSubagentSnapshot(serverId, snapshot, client);
    })
    .finally(() => pending?.delete(key));
  pending.set(key, request);
  return request;
}

/** A disconnected observation channel cannot confirm that native work is still running. */
export function markObservedSubagentsStale(serverId: string, parentAgentId: string): void {
  const session = useSessionStore.getState().sessions[serverId];
  const parent = session?.agents.get(parentAgentId) ?? session?.agentDetails.get(parentAgentId);
  const root = parent?.observation?.rootAgentId ?? parentAgentId;
  useSessionStore.getState().setAgents(serverId, (current) => {
    let next = current;
    for (const [id, agent] of current) {
      if (agent.observation?.rootAgentId !== root || agent.observation.health !== "current")
        continue;
      if (next === current) next = new Map(current);
      next.set(id, {
        ...agent,
        status: "closed",
        turn: { phase: "idle", cancellationRequestId: null },
        observation: { ...agent.observation, health: "stale" },
      });
    }
    return next;
  });
}

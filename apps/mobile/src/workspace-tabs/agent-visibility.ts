import type { Agent } from "@/stores/session-store";
import type { WorkspaceTabSnapshot } from "@/stores/workspace-layout-actions";
import { isWorkspaceRootAgent } from "@/subagents/policies";
import { normalizeWorkspaceOpaqueId } from "@/utils/workspace-identity";
import { areSetsEqual } from "@/utils/set-equality";

export interface WorkspaceAgentVisibility {
  activeAgentIds: Set<string>;
  autoOpenAgentIds: Set<string>;
}

function agentBelongsToWorkspace(agent: Agent, workspaceId: string): boolean {
  return normalizeWorkspaceOpaqueId(agent.workspaceId) === workspaceId;
}

export function deriveWorkspaceAgentVisibility(input: {
  sessionAgents: Map<string, Agent> | undefined;
  agentDetails?: Map<string, Agent> | undefined;
  workspaceId: string | null | undefined;
}): WorkspaceAgentVisibility {
  const { sessionAgents, agentDetails } = input;
  const workspaceId = normalizeWorkspaceOpaqueId(input.workspaceId);
  if ((!sessionAgents && !agentDetails) || !workspaceId) {
    return {
      activeAgentIds: new Set<string>(),
      autoOpenAgentIds: new Set<string>(),
    };
  }

  const activeAgentIds = new Set<string>();
  const autoOpenAgentIds = new Set<string>();
  for (const agent of sessionAgents?.values() ?? []) {
    if (!agentBelongsToWorkspace(agent, workspaceId)) {
      continue;
    }
    if (!agent.archivedAt) {
      activeAgentIds.add(agent.id);
      const parentAgent = agent.parentAgentId
        ? (sessionAgents?.get(agent.parentAgentId) ?? agentDetails?.get(agent.parentAgentId))
        : undefined;
      if (isWorkspaceRootAgent(agent, parentAgent)) {
        autoOpenAgentIds.add(agent.id);
      }
    }
  }
  return { activeAgentIds, autoOpenAgentIds };
}

interface AgentMapsSnapshot {
  sessions: Record<
    string,
    { agents: Map<string, Agent>; agentDetails: Map<string, Agent> } | undefined
  >;
}

// Session-store selectors run on every store update, including every stream tick, which
// leaves the agent maps untouched. Reuse the previous result until either map changes, and
// keep it when a recompute yields the same sets so the hook's equality check stays O(1).
export function createWorkspaceAgentVisibilitySelector(input: {
  serverId: string;
  workspaceId: string | null | undefined;
}): (state: AgentMapsSnapshot) => WorkspaceAgentVisibility {
  let previousAgents: Map<string, Agent> | undefined;
  let previousAgentDetails: Map<string, Agent> | undefined;
  let previousVisibility: WorkspaceAgentVisibility | null = null;

  return (state) => {
    const session = state.sessions[input.serverId];
    const agents = session?.agents;
    const agentDetails = session?.agentDetails;
    if (previousVisibility && agents === previousAgents && agentDetails === previousAgentDetails) {
      return previousVisibility;
    }

    const visibility = deriveWorkspaceAgentVisibility({
      sessionAgents: agents,
      agentDetails,
      workspaceId: input.workspaceId,
    });
    previousAgents = agents;
    previousAgentDetails = agentDetails;
    if (!previousVisibility || !workspaceAgentVisibilityEqual(previousVisibility, visibility)) {
      previousVisibility = visibility;
    }
    return previousVisibility;
  };
}

export function buildWorkspaceTabSnapshot(input: {
  agentVisibility: WorkspaceAgentVisibility;
  agentsHydrated: boolean;
  terminalsHydrated: boolean;
  knownTerminalIds: Iterable<string>;
  standaloneTerminalIds: Iterable<string>;
  hasActivePendingTerminalCreate: boolean;
  hasActivePendingDraftCreate: boolean;
}): WorkspaceTabSnapshot {
  return {
    agentsHydrated: input.agentsHydrated,
    terminalsHydrated: input.terminalsHydrated,
    activeAgentIds: input.agentVisibility.activeAgentIds,
    autoOpenAgentIds: input.agentVisibility.autoOpenAgentIds,
    knownTerminalIds: input.knownTerminalIds,
    standaloneTerminalIds: input.standaloneTerminalIds,
    hasActivePendingTerminalCreate: input.hasActivePendingTerminalCreate,
    hasActivePendingDraftCreate: input.hasActivePendingDraftCreate,
  };
}

export function workspaceAgentVisibilityEqual(
  a: WorkspaceAgentVisibility,
  b: WorkspaceAgentVisibility,
): boolean {
  return (
    a === b ||
    (areSetsEqual(a.activeAgentIds, b.activeAgentIds) &&
      areSetsEqual(a.autoOpenAgentIds, b.autoOpenAgentIds))
  );
}

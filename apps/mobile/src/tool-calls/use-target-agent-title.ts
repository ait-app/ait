import type { ToolCallDetail } from "@ait/protocol/agent-types";
import { getPaseoToolTargetAgentId } from "@ait/protocol/paseo-tool-presentation/index";
import { useSessionStore } from "@/stores/session-store";

type SessionStoreSnapshot = ReturnType<typeof useSessionStore.getState>;

interface TargetAgentInput {
  /** The host whose agent made the call. */
  serverId: string | undefined;
  toolName: string;
  detail: ToolCallDetail | undefined;
}

/** The host a call ran on: the one its input names with `hostId`, else the caller's own. */
function targetServerId({ serverId, detail }: TargetAgentInput): string | undefined {
  const input = detail?.type === "unknown" ? detail.input : undefined;
  const hostId =
    typeof input === "object" && input !== null && !Array.isArray(input)
      ? (input as Record<string, unknown>).hostId
      : undefined;
  return typeof hostId === "string" && hostId.trim() ? hostId.trim() : serverId;
}

/**
 * Title of the agent a Paseo tool call acts on, looked up on the host the call ran on. An agent
 * id belongs to its host, so a call routed to a host without a session here has no title.
 */
export function selectTargetAgentTitle(
  state: SessionStoreSnapshot,
  input: TargetAgentInput,
): string | undefined {
  const agentId = getPaseoToolTargetAgentId(input.toolName, input.detail);
  const serverId = targetServerId(input);
  if (!serverId || !agentId) return undefined;
  const session = state.sessions[serverId];
  const agent = session?.agents.get(agentId) ?? session?.agentDetails.get(agentId);
  return agent?.title ?? undefined;
}

/** Title of the agent a Paseo tool call acts on, when this host knows it. */
export function useTargetAgentTitle(input: TargetAgentInput): string | undefined {
  return useSessionStore((state) => selectTargetAgentTitle(state, input));
}

import type { AgentScreenMissingState } from "@/hooks/use-agent-screen-state-machine";

export function reconcileMissingAgentStateWithPresentAgent(
  state: AgentScreenMissingState,
): AgentScreenMissingState {
  if (state.kind === "resolving" || state.kind === "not_found") {
    return { kind: "idle" };
  }
  return state;
}

type AgentLookupTag = "idle" | "loading" | "not_found" | "error";

/** Only an idle lookup may fetch: a settled error waits for Retry, which resets to idle. */
export function shouldFetchAgentForLookup(tag: AgentLookupTag): boolean {
  return tag === "idle";
}

/** A lookup that failed while offline is worth one fresh attempt once the host is back. */
export function shouldRetryLookupOnConnectionChange(input: {
  tag: AgentLookupTag;
  wasConnected: boolean;
  isConnected: boolean;
}): boolean {
  return input.tag === "error" && !input.wasConnected && input.isConnected;
}

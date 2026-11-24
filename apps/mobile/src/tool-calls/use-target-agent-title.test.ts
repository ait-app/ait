import type { ToolCallDetail } from "@ait/protocol/agent-types";
import { describe, expect, it } from "vitest";
import type { useSessionStore } from "@/stores/session-store";
import { selectTargetAgentTitle } from "./use-target-agent-title";

type SessionStoreSnapshot = ReturnType<typeof useSessionStore.getState>;

/** Sessions by host, each with the titles of its agents by id. */
function sessions(hosts: Record<string, Record<string, string>>): SessionStoreSnapshot {
  const entries = Object.entries(hosts).map(([serverId, titles]) => [
    serverId,
    {
      agents: new Map(Object.entries(titles).map(([id, title]) => [id, { id, title }])),
      agentDetails: new Map(),
    },
  ]);
  return { sessions: Object.fromEntries(entries) } as unknown as SessionStoreSnapshot;
}

function call(input: Record<string, unknown>): ToolCallDetail {
  return { type: "unknown", input, output: null };
}

describe("selectTargetAgentTitle", () => {
  // The same agent id on two hosts names two different agents.
  const state = sessions({ caller: { "agt-1": "Local" }, other: { "agt-1": "Remote" } });
  const title = (input: Record<string, unknown>) =>
    selectTargetAgentTitle(state, {
      serverId: "caller",
      toolName: "mcp__ait__agent_get",
      detail: call(input),
    });

  it("looks the target up on the caller's host when the call names no host", () => {
    expect(title({ agentId: "agt-1" })).toBe("Local");
  });

  it("looks the target up on the host the call names with hostId", () => {
    expect(title({ agentId: "agt-1", hostId: "other" })).toBe("Remote");
  });

  it("shows no title for a host it has no session for, never the caller's agent", () => {
    expect(title({ agentId: "agt-1", hostId: "absent" })).toBeUndefined();
  });
});

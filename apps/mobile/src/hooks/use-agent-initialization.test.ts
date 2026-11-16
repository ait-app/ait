import { afterEach, describe, expect, it, vi } from "vitest";
import { useSessionStore } from "@/stores/session-store";
import {
  createInitDeferred,
  getInitDeferred,
  getInitKey,
  resolveInitDeferred,
} from "@/utils/agent-initialization";
import {
  createSetAgentInitializing,
  refreshAgentInitializationTimeout,
} from "./use-agent-initialization";

const serverId = "server-1";
const agentId = "agent-1";

function bindSetAgentInitializing() {
  return createSetAgentInitializing(serverId, useSessionStore.getState().setInitializingAgents);
}

function startInitialization(setAgentInitializing: ReturnType<typeof bindSetAgentInitializing>) {
  const key = getInitKey(serverId, agentId);
  const deferred = createInitDeferred(key, "tail");
  refreshAgentInitializationTimeout({ key, agentId, setAgentInitializing });
  setAgentInitializing(agentId, true);
  return deferred.promise;
}

afterEach(() => {
  resolveInitDeferred(getInitKey(serverId, agentId));
  useSessionStore.setState({ sessions: {}, agentLastActivity: new Map() });
  vi.restoreAllMocks();
});

describe("refreshAgentInitializationTimeout", () => {
  it("times out initialization after 65 seconds", async () => {
    vi.useFakeTimers();
    useSessionStore.getState().initializeSession(serverId, {} as never);

    const promise = startInitialization(bindSetAgentInitializing());

    vi.advanceTimersByTime(64_999);
    expect(getInitDeferred(getInitKey(serverId, agentId))).toBeDefined();

    vi.advanceTimersByTime(1);

    await expect(promise).rejects.toThrow("History sync timed out after 65s");
    expect(getInitDeferred(getInitKey(serverId, agentId))).toBeUndefined();
    expect(useSessionStore.getState().sessions[serverId]?.initializingAgents.get(agentId)).toBe(
      false,
    );
    vi.useRealTimers();
  });

  it("refreshes the initialization timeout after paged catch-up progress", async () => {
    vi.useFakeTimers();
    useSessionStore.getState().initializeSession(serverId, {} as never);
    const setAgentInitializing = bindSetAgentInitializing();
    const key = getInitKey(serverId, agentId);

    const promise = startInitialization(setAgentInitializing);

    vi.advanceTimersByTime(64_999);
    refreshAgentInitializationTimeout({ key, agentId, setAgentInitializing });

    vi.advanceTimersByTime(1);
    expect(getInitDeferred(key)).toBeDefined();

    const rejection = expect(promise).rejects.toThrow("History sync timed out after 65s");

    vi.advanceTimersByTime(64_998);
    expect(getInitDeferred(key)).toBeDefined();

    vi.advanceTimersByTime(1);

    await rejection;
    expect(getInitDeferred(key)).toBeUndefined();
    vi.useRealTimers();
  });
});

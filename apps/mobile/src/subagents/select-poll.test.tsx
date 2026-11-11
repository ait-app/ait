// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { afterEach, expect, test, vi } from "vitest";
import { useSessionStore, type Agent } from "@/stores/session-store";
import { useSubagentsForParent } from "./select";
import { refreshUnifiedSubagents } from "./unified";

vi.mock("@/hooks/use-archive-agent", () => ({
  usePendingArchiveAgentIds: () => new Set(),
}));
vi.mock("./unified", () => ({
  refreshUnifiedSubagents: vi.fn(async () => undefined),
  markObservedSubagentsStale: vi.fn(),
}));

const serverId = "opencode-subagent-poll";
const parentAgentId = "parent";

afterEach(() => {
  cleanup();
  useSessionStore.getState().clearSession(serverId);
  vi.useRealTimers();
  vi.clearAllMocks();
});

test("refreshes a running observed child and stops polling after completion", async () => {
  vi.useFakeTimers();
  const client = {
    isConnected: true,
    subscribeConnectionStatus: vi.fn(() => () => undefined),
  } as unknown as DaemonClient;
  const child = {
    id: "child",
    parentAgentId,
    provider: "opencode",
    status: "running",
    turn: { phase: "idle", cancellationRequestId: null },
    createdAt: new Date(0),
    capabilities: {},
    observation: { rootAgentId: parentAgentId },
  } as unknown as Agent;
  const store = useSessionStore.getState();
  store.initializeSession(serverId, client);
  store.setAgents(serverId, new Map([[child.id, child]]));
  useSessionStore.setState((state) => {
    const session = state.sessions[serverId];
    if (!session) return state;
    return {
      sessions: {
        ...state.sessions,
        [serverId]: {
          ...session,
          serverInfo: {
            serverId,
            hostname: "test",
            version: "test",
            features: { unifiedSubagentAgents: true },
          } as typeof session.serverInfo,
        },
      },
    };
  });
  renderHook(() => useSubagentsForParent({ serverId, parentAgentId }));

  await act(async () => vi.advanceTimersByTime(15_000));
  expect(refreshUnifiedSubagents).toHaveBeenCalledOnce();

  act(() => store.setAgents(serverId, new Map([[child.id, { ...child, status: "idle" }]])));
  await act(async () => vi.advanceTimersByTime(30_000));
  expect(refreshUnifiedSubagents).toHaveBeenCalledOnce();
});

// @vitest-environment jsdom
import { act, cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { DaemonClient } from "@ait/client/internal/daemon-client";
import type { SessionOutboundMessage } from "@ait/protocol/messages";
import { useSessionStore } from "@/stores/session-store";
import {
  createViewedTimelineOwner,
  type ViewedTimelineOwnerPorts,
} from "@/timeline/viewed-timeline-sync";
import { SessionProvider } from "./session-context";

const host = vi.hoisted(() => ({
  connected: true,
  fetchAgentTimeline: vi.fn(),
  createViewedTimelineOwner: vi.fn(),
}));

vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeIsConnected: () => host.connected,
  getHostRuntimeStore: () => host,
}));
vi.mock("@tanstack/react-query", () => ({ useQueryClient: () => null }));
vi.mock("@/hooks/use-client-activity", () => ({ useClientActivity: () => {} }));
vi.mock("@/hooks/use-app-visible", () => ({ useAppVisible: () => true }));
vi.mock("@/push-notifications", () => ({ startPushNotifications: () => () => {} }));
vi.mock("@/contexts/voice-context", () => ({
  useVoiceRuntimeOptional: () => null,
  useVoiceAudioEngineOptional: () => null,
}));
vi.mock("@/contexts/toast-context", () => ({ useToast: () => ({ error: vi.fn() }) }));
vi.mock("@/stores/workspace-layout-store", () => ({
  observeOpenWorkspaceAgentIds: () => () => {},
}));
vi.mock("@/git/checkout-status-cache", () => ({ applyCheckoutStatusUpdateFromEvent: vi.fn() }));
vi.mock("@/subagents/provider-store", () => ({
  resyncProviderSubagents: vi.fn(),
  useProviderSubagentStore: { getState: vi.fn() },
}));

const serverId = "server";
const agentId = "agent";

function clientFixture() {
  const client = new DaemonClient({ url: "ws://unused", clientId: "test" });
  type Observation = ReturnType<DaemonClient["observeTimeline"]>;
  let listener: Parameters<Observation["subscribe"]>[0] | undefined;
  const observation = {
    subscriptionId: "timeline",
    ready: Promise.resolve({
      requestId: "subscribe",
      subscriptionId: "timeline",
      agentIds: [agentId],
    }),
    subscribe: vi.fn((observer: Parameters<Observation["subscribe"]>[0]) => {
      listener = observer;
      return () => {
        listener = undefined;
      };
    }),
    release: vi.fn(async () => {}),
  };
  vi.spyOn(client, "observeEvents").mockReturnValue({
    ...observation,
    subscribe: () => () => {},
  });
  vi.spyOn(client, "observeTimeline").mockReturnValue(observation);
  return {
    client,
    observation,
    emit: (message: SessionOutboundMessage) => listener?.update(message),
  };
}

afterEach(() => {
  cleanup();
  useSessionStore.setState({ sessions: {}, agentLastActivity: new Map() });
  vi.restoreAllMocks();
  vi.clearAllMocks();
});

test("a replacement client catches up and streams while the host stays connected", async () => {
  host.connected = true;
  useSessionStore.getState().initializeSession(serverId, null);
  host.createViewedTimelineOwner.mockImplementation((id: string, ports: ViewedTimelineOwnerPorts) =>
    createViewedTimelineOwner({
      serverId: id,
      ports,
      replica: {
        prepare: async () => {},
        readCursor: () => undefined,
        readRange: () => undefined,
        timelineUpdated: () => {},
      },
      replaceDemandedAgentIds: () => {},
      drainQueuedAgentMessage: () => {},
    }),
  );
  host.fetchAgentTimeline.mockResolvedValue({ hasNewer: false, endCursor: null });
  const first = clientFixture();
  const next = clientFixture();
  const view = render(
    <SessionProvider serverId={serverId} client={first.client}>
      {null}
    </SessionProvider>,
  );
  const owner = () => useSessionStore.getState().sessions[serverId]!.viewedTimelineSync!;
  act(() => owner().replaceVisibleAgentIds("chat", [agentId]));
  await waitFor(() => expect(owner().getAgentTimelineStatus(agentId)).toBe("ready"));
  expect(host.fetchAgentTimeline).toHaveBeenCalledTimes(1);
  const previousOwner = owner();

  view.rerender(
    <SessionProvider serverId={serverId} client={next.client}>
      {null}
    </SessionProvider>,
  );
  expect(owner()).not.toBe(previousOwner);
  act(() => owner().replaceVisibleAgentIds("chat", [agentId]));

  await waitFor(() => expect(next.client.observeTimeline).toHaveBeenCalledWith([agentId]));
  await waitFor(() => expect(host.fetchAgentTimeline).toHaveBeenCalledTimes(2));
  expect(first.observation.release).toHaveBeenCalled();
  expect(owner().getAgentTimelineStatus(agentId)).toBe("ready");

  act(() => {
    next.emit({
      type: "agent.stream",
      payload: {
        agentId,
        epoch: "epoch",
        seq: 1,
        timestamp: "2026-10-30T16:00:00Z",
        event: {
          type: "timeline",
          provider: "codex",
          item: { type: "assistant_message", text: "New reply after connection switch" },
        },
      },
    });
    owner().flushStreamAgent(agentId);
  });
  expect(useSessionStore.getState().sessions[serverId]!.agentStreamHead.get(agentId)).toEqual(
    expect.arrayContaining([
      expect.objectContaining({
        kind: "assistant_message",
        text: "New reply after connection switch",
      }),
    ]),
  );

  const currentOwner = owner();
  view.rerender(
    <SessionProvider serverId={serverId} client={next.client}>
      <span>Unrelated render</span>
    </SessionProvider>,
  );
  expect(owner()).toBe(currentOwner);
  expect(next.client.observeTimeline).toHaveBeenCalledTimes(1);
  expect(host.fetchAgentTimeline).toHaveBeenCalledTimes(2);

  host.connected = false;
  view.rerender(
    <SessionProvider serverId={serverId} client={next.client}>
      {null}
    </SessionProvider>,
  );
  expect(owner().getAgentTimelineStatus(agentId)).toBe("pending");
  expect(next.client.observeTimeline).toHaveBeenCalledTimes(1);
  host.connected = true;
  view.rerender(
    <SessionProvider serverId={serverId} client={next.client}>
      {null}
    </SessionProvider>,
  );
  await waitFor(() => expect(owner().getAgentTimelineStatus(agentId)).toBe("ready"));
  expect(next.client.observeTimeline).toHaveBeenCalledTimes(2);
  expect(host.fetchAgentTimeline).toHaveBeenCalledTimes(3);
});

import { afterEach, describe, expect, it, vi } from "vitest";
import type { AgentSnapshotPayload, AgentStreamEventPayload } from "@ait/protocol/messages";
import { selectAgentTurnPresentation, useSessionStore } from "@/stores/session-store";
import { normalizeAgentSnapshot } from "@/utils/agent-snapshots";
import { createUserMessage } from "@/types/stream";
import {
  createSessionAgentStreamReducerQueue,
  processTimelineResponse,
} from "./session-stream-reducers";

const serverId = "stream-submission-receipts";
const agentId = "agent";
const clientMessageId = "submitted-prompt";
const timestamp = new Date("2026-11-26T09:32:45.000Z");
const cursor = { epoch: "epoch-1", startSeq: 20, endSeq: 40 };
const idle: AgentSnapshotPayload = {
  id: agentId,
  provider: "codex",
  cwd: "/repo",
  model: null,
  createdAt: timestamp.toISOString(),
  updatedAt: timestamp.toISOString(),
  lastUserMessageAt: timestamp.toISOString(),
  status: "idle",
  activeTurn: null,
  capabilities: {
    supportsStreaming: true,
    supportsSessionPersistence: true,
    supportsDynamicModes: false,
    supportsMcpServers: false,
    supportsReasoningStream: true,
    supportsToolInvocations: true,
  },
  currentModeId: null,
  availableModes: [],
  pendingPermissions: [],
  persistence: null,
  title: null,
  labels: {},
};

const receipt: AgentStreamEventPayload = {
  type: "timeline",
  provider: "codex",
  turnId: "finished-turn",
  item: { type: "user_message", text: "Check the models", clientMessageId },
};

afterEach(() => useSessionStore.getState().clearSession(serverId));

function prepareSubmission(detached: boolean) {
  const store = useSessionStore.getState();
  store.initializeSession(serverId, null);
  store.setAgents(serverId, new Map([[agentId, normalizeAgentSnapshot(idle, serverId)]]));
  store.beginAgentMessageSubmission(
    serverId,
    agentId,
    createUserMessage({ clientMessageId, text: "Check the models", timestamp }),
  );
  // A history window can replace the optimistic row before its receipt is reduced.
  store.applyAgentTimelineResponseState(serverId, agentId, {
    items: [createUserMessage({ id: "historical", text: "Earlier prompt", timestamp })],
    head: [],
    range: cursor,
    older: "available",
    newer: detached,
    synchronized: true,
    acknowledgedClientMessageIds: [],
  });
  return createSessionAgentStreamReducerQueue({
    serverId,
    setAgentStreamState: store.setAgentStreamState,
    setAgentTimelineCursor: store.setAgentTimelineCursor,
    recoverTimelineGap: vi.fn(),
  });
}

describe("submission receipts independent of transcript rendering", () => {
  const modes = [
    { name: "a newly inserted canonical row", seq: 41, epoch: "epoch-1", detached: false },
    { name: "a replay already covered by history", seq: 30, epoch: "epoch-1", detached: false },
    { name: "an event awaiting gap recovery", seq: 70, epoch: "epoch-1", detached: false },
    { name: "an event from a replaced epoch", seq: 70, epoch: "epoch-2", detached: false },
    { name: "a receipt outside the viewed window", seq: 70, epoch: "epoch-1", detached: true },
  ];

  it.each(modes.flatMap((mode) => [true, false].map((rpcFirst) => ({ ...mode, rpcFirst }))))(
    "settles an idle agent after $name (RPC first: $rpcFirst)",
    ({ seq, epoch, detached, rpcFirst }) => {
      const queue = prepareSubmission(detached);
      const store = useSessionStore.getState();
      if (rpcFirst) store.acceptAgentMessageSubmission(serverId, agentId, clientMessageId);
      const before = store.getSession(serverId)!;
      expect(selectAgentTurnPresentation(before, agentId)).toMatchObject({
        isActive: true,
        startedAt: null,
      });

      queue.enqueue(agentId, { event: receipt, seq, epoch, timestamp });
      queue.flush();
      queue.dispose();

      const after = store.getSession(serverId)!;
      expect(selectAgentTurnPresentation(after, agentId)).toMatchObject({
        isActive: false,
        isCancelling: false,
      });
      if (seq !== 41) {
        expect(after.agentStreamTail.get(agentId)).toBe(before.agentStreamTail.get(agentId));
        expect(after.agentStreamHead.get(agentId)).toBe(before.agentStreamHead.get(agentId));
        expect(after.agentTimelineCursor.get(agentId)).toBe(
          before.agentTimelineCursor.get(agentId),
        );
      }
      if (!rpcFirst) store.acceptAgentMessageSubmission(serverId, agentId, clientMessageId);
      expect(store.getSession(serverId)?.messageSubmissions.has(agentId)).toBe(false);
    },
  );

  it("keeps a different unacknowledged submission active", () => {
    const queue = prepareSubmission(true);
    const store = useSessionStore.getState();
    store.beginAgentMessageSubmission(
      serverId,
      agentId,
      createUserMessage({ clientMessageId: "next-prompt", text: "Next question", timestamp }),
    );
    queue.enqueue(agentId, { event: receipt, seq: 70, epoch: "epoch-1", timestamp });
    queue.flush();
    queue.dispose();

    const session = store.getSession(serverId)!;
    expect(session.messageSubmissions.get(agentId)).toEqual([
      { clientMessageId, providerAcknowledged: true, rpcSettled: false },
      { clientMessageId: "next-prompt", providerAcknowledged: false, rpcSettled: false },
    ]);
    expect(selectAgentTurnPresentation(session, agentId).isActive).toBe(true);
  });

  it("keeps an actual running turn active after acknowledging its prompt", () => {
    const queue = prepareSubmission(true);
    const store = useSessionStore.getState();
    store.setAgents(
      serverId,
      new Map([
        [
          agentId,
          normalizeAgentSnapshot(
            {
              ...idle,
              status: "running",
              activeTurn: { turnId: "active-turn", startedAt: timestamp.toISOString() },
            },
            serverId,
          ),
        ],
      ]),
    );
    queue.enqueue(agentId, { event: receipt, seq: 70, epoch: "epoch-1", timestamp });
    queue.flush();
    queue.dispose();

    expect(selectAgentTurnPresentation(store.getSession(serverId), agentId)).toMatchObject({
      isActive: true,
      turnId: "active-turn",
    });
    expect(
      store.getSession(serverId)?.messageSubmissions.get(agentId)?.[0]?.providerAcknowledged,
    ).toBe(true);
  });

  it.each([
    { direction: "tail", reset: false },
    { direction: "tail", reset: true },
    { direction: "after", reset: false },
    { direction: "before", reset: false },
    { direction: "tail", reset: false, mergeWindow: true },
  ] as const)(
    "recovers a missed receipt from history ($direction, reset: $reset, merge: $mergeWindow)",
    (page) => {
      prepareSubmission(true).dispose();
      const store = useSessionStore.getState();
      store.acceptAgentMessageSubmission(serverId, agentId, clientMessageId);
      const before = store.getSession(serverId)!;
      const result = processTimelineResponse({
        payload: {
          agentId,
          ...page,
          projection: "projected",
          epoch: "epoch-1",
          window: { minSeq: 1, maxSeq: 40, nextSeq: 41 },
          startCursor: { seq: 30 },
          endCursor: { seq: 30 },
          entries: [
            {
              seqStart: 30,
              seqEnd: 30,
              provider: "codex",
              item: { type: "user_message", text: "Check the models", clientMessageId },
              timestamp: timestamp.toISOString(),
            },
          ],
          error: null,
          hasOlder: true,
          hasNewer: false,
        },
        currentTail: before.agentStreamTail.get(agentId)!,
        currentHead: [],
        currentCursor: cursor,
        isInitializing: false,
        hasActiveInitDeferred: false,
        initRequestDirection: "tail",
        sendingClientMessageIds: [clientMessageId],
      });
      store.setAgentStreamState(serverId, agentId, {
        acknowledgedClientMessageIds: result.acknowledgedClientMessageIds,
      });

      expect(result.acknowledgedClientMessageIds).toEqual([clientMessageId]);
      expect(selectAgentTurnPresentation(store.getSession(serverId), agentId).isActive).toBe(false);
    },
  );

  it("does not infer a receipt from matching text outside the viewed window", () => {
    const queue = prepareSubmission(true);
    const event: AgentStreamEventPayload = {
      type: "timeline",
      provider: "codex",
      item: { type: "user_message", text: "Check the models" },
    };
    queue.enqueue(agentId, { event, seq: 70, epoch: "epoch-1", timestamp });
    queue.enqueue(agentId, {
      event: { type: "turn_completed", provider: "codex", turnId: "finished-turn" },
      seq: undefined,
      epoch: undefined,
      timestamp,
    });
    queue.flush();
    queue.dispose();

    expect(
      selectAgentTurnPresentation(useSessionStore.getState().getSession(serverId), agentId)
        .isActive,
    ).toBe(true);
  });
});

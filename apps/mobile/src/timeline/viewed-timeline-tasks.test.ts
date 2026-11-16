import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { useSessionStore } from "@/stores/session-store";
import type { TodoEntry } from "@/types/stream";
import {
  createViewedTimelineOwner,
  type TimelineResponsePayload,
  type ViewedTimelineOwner,
} from "./viewed-timeline-sync";

const serverId = "current-todo-snapshot";
const agentId = "agent";
const epoch = "epoch-current";
let owner: ViewedTimelineOwner;

function response(overrides: Partial<TimelineResponsePayload> = {}): TimelineResponsePayload {
  return {
    requestId: "request",
    agentId,
    agent: null,
    direction: "tail",
    projection: "canonical",
    epoch,
    reset: false,
    staleCursor: false,
    gap: false,
    window: { minSeq: 1, maxSeq: 100, nextSeq: 101 },
    startCursor: { epoch, seq: 100 },
    endCursor: { epoch, seq: 100 },
    hasOlder: true,
    hasNewer: false,
    entries: [
      {
        sourceSeqRanges: [],
        collapsed: [],
        seqStart: 100,
        seqEnd: 100,
        provider: "codex",
        item: { type: "assistant_message", text: "Current reply" },
        timestamp: new Date(100_000).toISOString(),
      },
    ],
    error: null,
    ...overrides,
  };
}

const tasks: TodoEntry[] = [{ text: "Task outside the transcript page", completed: false }];
const currentTasks = () => useSessionStore.getState().sessions[serverId]?.agentTasks.get(agentId);
const taskCursor = () =>
  useSessionStore.getState().sessions[serverId]?.agentTaskCursor.get(agentId);

function liveTodo(seq: number, items: TodoEntry[]): void {
  owner.enqueueStreamEvent(agentId, {
    event: { type: "timeline", provider: "codex", item: { type: "todo", items } },
    seq,
    epoch,
    timestamp: new Date(seq * 1000),
  });
  owner.flushStreamAgent(agentId);
}

beforeEach(() => {
  useSessionStore.getState().initializeSession(serverId, null as unknown as DaemonClient);
  owner = createViewedTimelineOwner({
    serverId,
    replica: {
      prepare: async () => {},
      readCursor: () => undefined,
      readRange: () => undefined,
      timelineUpdated: vi.fn(),
    },
    replaceDemandedAgentIds: vi.fn(),
    drainQueuedAgentMessage: vi.fn(),
    ports: {
      observe: () => ({ ready: Promise.resolve(), release: async () => {} }),
      readCursor: () => undefined,
      fetchPage: async () => ({ hasNewer: false, endCursor: null }),
      fetchLatestTail: async () => ({ hasNewer: false, endCursor: null }),
      reportError: vi.fn(),
      schedule: (task, delay) => {
        const timer = setTimeout(task, delay);
        return () => clearTimeout(timer);
      },
    },
  });
});

afterEach(() => {
  owner.dispose();
  useSessionStore.getState().clearSession(serverId);
});

describe("current TODO state independent of transcript pages", () => {
  it("restores a TODO snapshot before the first row in a cold latest page", () => {
    owner.applyTimelineResponse(response({ taskSnapshot: { epoch, seq: 2, items: tasks } }));

    expect(currentTasks()).toEqual(tasks);
    expect(taskCursor()).toEqual({ epoch, seq: 100 });
    expect(useSessionStore.getState().sessions[serverId]?.agentStreamTail.get(agentId)).not.toEqual(
      expect.arrayContaining([expect.objectContaining({ kind: "todo_list" })]),
    );
  });

  it("retains current tasks while the user browses older transcript windows", () => {
    owner.applyTimelineResponse(response({ taskSnapshot: { epoch, seq: 2, items: tasks } }));
    owner.applyTimelineResponse(
      response({
        direction: "before",
        startCursor: { epoch, seq: 20 },
        endCursor: { epoch, seq: 20 },
        entries: [
          {
            sourceSeqRanges: [],
            collapsed: [],
            seqStart: 20,
            seqEnd: 20,
            provider: "codex",
            item: { type: "todo", items: [{ text: "Historical task", completed: true }] },
            timestamp: new Date(20_000).toISOString(),
          },
        ],
        taskSnapshot: { epoch, seq: 90, items: tasks },
        hasNewer: true,
      }),
    );

    expect(currentTasks()).toEqual(tasks);
    expect(taskCursor()).toEqual({ epoch, seq: 100 });
  });

  it.each([null, { epoch, seq: 2, items: tasks }])(
    "does not overwrite a newer live TODO with an older response snapshot (%j)",
    (taskSnapshot) => {
      owner.applyTimelineResponse(response({ taskSnapshot: { epoch, seq: 2, items: tasks } }));
      const latest = [{ text: "Newest live TODO", completed: false }];
      liveTodo(101, latest);

      owner.applyTimelineResponse(response({ taskSnapshot }));

      expect(currentTasks()).toEqual(latest);
      expect(taskCursor()).toEqual({ epoch, seq: 101 });
    },
  );

  it.each([null, { epoch, seq: 102, items: [] }])(
    "clears tasks from an authoritative current empty snapshot (%j)",
    (taskSnapshot) => {
      owner.applyTimelineResponse(response({ taskSnapshot: { epoch, seq: 2, items: tasks } }));
      owner.applyTimelineResponse(
        response({
          taskSnapshot,
          window: { minSeq: 1, maxSeq: 102, nextSeq: 103 },
        }),
      );

      expect(currentTasks()).toBeUndefined();
      expect(taskCursor()).toEqual({ epoch, seq: 102 });
    },
  );

  it("keeps an older transcript fixed while current canonical TODO snapshots advance", () => {
    owner.applyTimelineResponse(response({ taskSnapshot: { epoch, seq: 2, items: tasks } }));
    useSessionStore.getState().setAgentTimelineHasNewer(serverId, new Map([[agentId, true]]));
    const before = useSessionStore.getState().sessions[serverId]!;
    const tail = before.agentStreamTail.get(agentId);
    const cursor = before.agentTimelineCursor.get(agentId);
    const latest = [{ text: "Current work while reading history", completed: false }];

    liveTodo(120, latest);
    liveTodo(119, tasks);

    const after = useSessionStore.getState().sessions[serverId]!;
    expect(after.agentStreamTail.get(agentId)).toBe(tail);
    expect(after.agentTimelineCursor.get(agentId)).toBe(cursor);
    expect(currentTasks()).toEqual(latest);
    expect(taskCursor()).toEqual({ epoch, seq: 120 });
  });

  it.each(["before", "after"] as const)(
    "ignores a delayed foreign-epoch %s snapshot or null while keeping the current checklist",
    (direction) => {
      owner.applyTimelineResponse(response({ taskSnapshot: { epoch, seq: 2, items: tasks } }));
      for (const taskSnapshot of [
        null,
        {
          epoch: "retired-epoch",
          seq: 2,
          items: [{ text: "Retired checklist", completed: false }],
        },
      ]) {
        owner.applyTimelineResponse(
          response({
            direction,
            epoch: "retired-epoch",
            taskSnapshot,
            startCursor: { epoch: "retired-epoch", seq: 50 },
            endCursor: { epoch: "retired-epoch", seq: 50 },
            window: { minSeq: 1, maxSeq: 200, nextSeq: 201 },
            entries: [
              {
                seqStart: 50,
                seqEnd: 50,
                sourceSeqRanges: [],
                collapsed: [],
                provider: "codex",
                item: { type: "assistant_message", text: "Retired transcript" },
                timestamp: new Date(50_000).toISOString(),
              },
            ],
          }),
        );
        expect(currentTasks()).toEqual(tasks);
        expect(taskCursor()).toEqual({ epoch, seq: 100 });
      }
    },
  );

  it("accepts a current TODO snapshot when a tail response replaces the epoch", () => {
    owner.applyTimelineResponse(response({ taskSnapshot: { epoch, seq: 2, items: tasks } }));
    const replacement = [{ text: "Replacement generation task", completed: false }];
    owner.applyTimelineResponse(
      response({
        epoch: "replacement",
        reset: true,
        taskSnapshot: { epoch: "replacement", seq: 1, items: replacement },
        startCursor: { epoch: "replacement", seq: 2 },
        endCursor: { epoch: "replacement", seq: 2 },
        window: { minSeq: 1, maxSeq: 2, nextSeq: 3 },
        entries: [
          {
            seqStart: 2,
            seqEnd: 2,
            sourceSeqRanges: [],
            collapsed: [],
            provider: "codex",
            item: { type: "assistant_message", text: "Replacement transcript" },
            timestamp: new Date(2_000).toISOString(),
          },
        ],
      }),
    );
    expect(currentTasks()).toEqual(replacement);
    expect(taskCursor()).toEqual({ epoch: "replacement", seq: 2 });
  });

  it("reads a large current TODO snapshot by reference to the response's canonical row", () => {
    const largeTasks = [{ text: "Native task details ".repeat(30_000), completed: false }];
    owner.applyTimelineResponse(
      response({
        taskSnapshot: { epoch, seq: 100, entrySeq: 100 },
        entries: [
          {
            seqStart: 100,
            seqEnd: 100,
            sourceSeqRanges: [],
            collapsed: [],
            provider: "codex",
            item: { type: "todo", items: largeTasks },
            timestamp: new Date(100_000).toISOString(),
          },
        ],
      }),
    );
    expect(currentTasks()).toEqual(largeTasks);
    expect(taskCursor()).toEqual({ epoch, seq: 100 });
  });

  it.each([99, 200])(
    "keeps current tasks when snapshot reference %s has no matching TODO row",
    (entrySeq) => {
      owner.applyTimelineResponse(response({ taskSnapshot: { epoch, seq: 2, items: tasks } }));
      owner.applyTimelineResponse(
        response({
          hasOlder: false,
          window: { minSeq: 1, maxSeq: 200, nextSeq: 201 },
          startCursor: { epoch, seq: 200 },
          endCursor: { epoch, seq: 200 },
          entries: [
            {
              seqStart: 200,
              seqEnd: 200,
              sourceSeqRanges: [],
              collapsed: [],
              provider: "codex",
              item: { type: "assistant_message", text: "Newer transcript" },
              timestamp: new Date(200_000).toISOString(),
            },
          ],
          taskSnapshot: { epoch, seq: entrySeq, entrySeq },
        }),
      );
      expect(currentTasks()).toEqual(tasks);
      expect(taskCursor()).toEqual({ epoch, seq: 100 });
    },
  );

  it("retains legacy daemon support by reading TODO rows when no snapshot field exists", () => {
    owner.applyTimelineResponse(
      response({
        entries: [
          {
            sourceSeqRanges: [],
            collapsed: [],
            seqStart: 100,
            seqEnd: 100,
            provider: "codex",
            item: { type: "todo", items: tasks },
            timestamp: new Date(100_000).toISOString(),
          },
        ],
      }),
    );

    expect(currentTasks()).toEqual(tasks);
  });
});

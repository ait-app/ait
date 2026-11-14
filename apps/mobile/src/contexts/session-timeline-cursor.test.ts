import { afterEach, describe, expect, it } from "vitest";
import { useSessionStore } from "@/stores/session-store";
import { TIMELINE_FETCH_PAGE_SIZE } from "@/timeline/timeline-fetch-policy";
import { planTimelineResumeFetch } from "@/timeline/timeline-sync-plan";
import { readSyncedTimelineCursor } from "./session-timeline-cursor";

const serverId = "server-1";
const agentId = "agent-1";

// The composition the viewed-timeline owner runs on resume: session-context's `readCursor`
// port feeding `planTimelineResumeFetch` in `timeline/viewed-timeline-sync.ts`.
function planResume() {
  return planTimelineResumeFetch(
    readSyncedTimelineCursor(useSessionStore.getState().sessions[serverId], agentId),
  );
}

afterEach(() => {
  useSessionStore.setState({ sessions: {}, agentLastActivity: new Map() });
});

describe("readSyncedTimelineCursor resume plan", () => {
  it("catches up after loaded authoritative history", () => {
    useSessionStore.getState().initializeSession(serverId, {} as never);
    useSessionStore
      .getState()
      .setAgentTimelineCursor(
        serverId,
        new Map([[agentId, { epoch: "epoch-1", startSeq: 1, endSeq: 42 }]]),
      );
    useSessionStore.getState().setAgentAuthoritativeHistoryApplied(serverId, agentId, true);

    expect(planResume()).toEqual({
      direction: "after",
      cursor: { epoch: "epoch-1", seq: 42 },
      limit: TIMELINE_FETCH_PAGE_SIZE,
      projection: "projected",
    });
  });

  it("requests a bounded projected tail when no authoritative cursor is available", () => {
    useSessionStore.getState().initializeSession(serverId, {} as never);

    expect(planResume()).toEqual({
      direction: "tail",
      limit: TIMELINE_FETCH_PAGE_SIZE,
      projection: "projected",
    });
  });

  it("requests a bounded projected tail after restoring painted replica items", () => {
    useSessionStore.getState().initializeSession(serverId, null);
    useSessionStore.getState().applyAgentTimelineResponseState(serverId, agentId, {
      items: [
        {
          kind: "assistant_message",
          id: "painted-item",
          text: "Painted before hydration",
          timestamp: new Date("2026-07-27T10:00:00.000Z"),
        },
      ],
      head: [],
      range: null,
      older: "none",
      newer: false,
      synchronized: false,
      acknowledgedClientMessageIds: [],
    });

    expect(planResume()).toEqual({
      direction: "tail",
      limit: TIMELINE_FETCH_PAGE_SIZE,
      projection: "projected",
    });
  });

  // The replica cache restores painted rows together with their stored range but leaves them
  // unsynchronized (`timeline/viewed-timeline-sync.ts`); that range is not a confirmed cursor.
  it("requests a bounded projected tail when the restored range is not synchronized", () => {
    useSessionStore.getState().initializeSession(serverId, null);
    useSessionStore.getState().applyAgentTimelineResponseState(serverId, agentId, {
      items: [
        {
          kind: "assistant_message",
          id: "painted-item",
          text: "Painted from the replica cache",
          timelineCursor: { epoch: "epoch-1", seq: 12 },
          timestamp: new Date("2026-07-27T10:00:00.000Z"),
        },
      ],
      head: [],
      range: { epoch: "epoch-1", startSeq: 10, endSeq: 12 },
      older: "available",
      newer: false,
      synchronized: false,
      acknowledgedClientMessageIds: [],
    });

    expect(planResume()).toEqual({
      direction: "tail",
      limit: TIMELINE_FETCH_PAGE_SIZE,
      projection: "projected",
    });
  });

  it("catches up after the restored canonical replica range", () => {
    useSessionStore.getState().initializeSession(serverId, null);
    useSessionStore.getState().applyAgentTimelineResponseState(serverId, agentId, {
      items: [
        {
          kind: "assistant_message",
          id: "canonical-item",
          text: "Canonical before restart",
          timelineCursor: { epoch: "epoch-1", seq: 12 },
          timestamp: new Date("2026-07-27T10:00:00.000Z"),
        },
      ],
      head: [],
      range: { epoch: "epoch-1", startSeq: 10, endSeq: 12 },
      older: "available",
      newer: false,
      synchronized: true,
      acknowledgedClientMessageIds: [],
    });

    expect(planResume()).toEqual({
      direction: "after",
      cursor: { epoch: "epoch-1", seq: 12 },
      limit: TIMELINE_FETCH_PAGE_SIZE,
      projection: "projected",
    });
  });
});

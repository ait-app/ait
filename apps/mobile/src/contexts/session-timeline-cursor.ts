import { selectAgentTimelineState, type SessionState } from "@/stores/session-store";

/**
 * The cursor a viewed timeline resumes from: the end of the authoritative range, and only once
 * the agent's history is synchronized. Painted-only replica items carry no cursor, so the resume
 * plan falls back to a bounded tail fetch.
 */
export function readSyncedTimelineCursor(
  session: SessionState | undefined,
  agentId: string,
): { epoch: string; endSeq: number } | undefined {
  const timeline = selectAgentTimelineState(session, agentId);
  return timeline.status === "synced" && timeline.range
    ? { epoch: timeline.range.epoch, endSeq: timeline.range.endSeq }
    : undefined;
}

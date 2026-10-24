import { expect, it } from "vitest";
import type { VoiceTimelineItem } from "@ait/protocol/agent-types";
import {
  AgentTimelineItemPayloadSchema,
  type AgentStreamEventPayload,
} from "@ait/protocol/messages";
import {
  applyStreamEvent,
  hydrateStreamState,
  replaceWithCanonicalStream,
  type StreamItem,
} from "./stream";
import { continuesResponse, continuesTurn } from "@/agent-stream/turn-membership";

function event(item: VoiceTimelineItem): AgentStreamEventPayload {
  return { type: "timeline", provider: "codex", item: AgentTimelineItemPayloadSchema.parse(item) };
}

it("replaces interleaved speech snapshots and final corrections without creating text turns", () => {
  const base = {
    type: "voice",
    sessionId: "call",
    kind: "transcript",
    status: "streaming",
  } as const;
  const items: VoiceTimelineItem[] = [
    { ...base, id: "u", role: "user", text: "hel" },
    { ...base, id: "a", role: "assistant", text: "hi" },
    { ...base, id: "u", role: "user", text: "hello" },
    { ...base, id: "u", role: "user", text: "Hello!", status: "completed" },
    { ...base, id: "a", role: "assistant", text: "Hi there!", status: "completed" },
    { type: "voice", sessionId: "call", kind: "session", id: "end", status: "ended" },
  ];
  let tail: StreamItem[] = [];
  let head: StreamItem[] = [];
  items.forEach((item, i) => {
    const next = applyStreamEvent({ tail, head, event: event(item), timestamp: new Date(i + 1) });
    tail = next.tail;
    head = next.head;
  });
  const replay = hydrateStreamState(
    items.map((item, i) => ({ event: event(item), timestamp: new Date(i + 1) })),
  );
  expect([...tail, ...head]).toEqual(replay);
  expect(replay).toHaveLength(3);
  expect(replay.map((item) => item.kind === "voice" && item.payload)).toEqual([
    items[3],
    items[4],
    items[5],
  ]);
  expect(replay[0].timestamp).toEqual(new Date(1));
  expect(replay.every((item) => item.turnId === undefined)).toBe(true);
});

it("keeps equal speech in different calls as distinct utterances", () => {
  const items: VoiceTimelineItem[] = ["one", "two"].map((sessionId) => ({
    type: "voice",
    sessionId,
    kind: "transcript",
    id: "same",
    role: "user",
    text: "hello",
    status: "completed",
  }));
  const replay = hydrateStreamState(
    items.map((item) => ({ event: event(item), timestamp: new Date(1) })),
  );
  expect(replay).toHaveLength(2);
  expect(replay[0].id).not.toBe(replay[1].id);
});

it("retains a newer live voice snapshot when an older history page arrives", () => {
  const payload: VoiceTimelineItem = {
    type: "voice",
    kind: "transcript",
    id: "u",
    sessionId: "call",
    role: "user",
    text: "hel",
    status: "streaming",
  };
  const old: StreamItem = {
    kind: "voice",
    id: "voice:call/u",
    timestamp: new Date(1),
    timelineCursor: { epoch: "epoch", seq: 1 },
    payload,
  };
  const latest: StreamItem = {
    ...old,
    timelineCursor: { epoch: "epoch", seq: 3 },
    payload: { ...payload, text: "Hello!", status: "completed" },
  };
  const result = replaceWithCanonicalStream({
    canonical: [old],
    previousTail: [latest],
    previousHead: [],
    sendingClientMessageIds: [],
    preserveContinuity: true,
    canonicalCoverage: { epoch: "epoch", endSeq: 1 },
  });
  expect(result.tail).toEqual([latest]);
  expect(result.head).toEqual([]);
  const late = applyStreamEvent({
    tail: result.tail,
    head: [],
    event: event(payload),
    timestamp: new Date(4),
  });
  expect(late.tail).toEqual([latest]);
  const assistant: StreamItem = {
    kind: "assistant_message",
    id: "answer",
    text: "answer",
    timestamp: new Date(1),
    turnId: "turn",
  };
  expect(continuesTurn(assistant, latest)).toBe(false);
  expect(continuesResponse(latest, assistant)).toBe(false);
});

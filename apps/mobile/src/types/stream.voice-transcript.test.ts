import { expect, it } from "vitest";
import type { AgentStreamEventPayload } from "@ait/protocol/messages";
import type { VoiceTranscriptEntry } from "@ait/protocol/agent-types";
import { applyStreamEvent, createUserMessage, hydrateStreamState } from "./stream";

const voiceTranscript: VoiceTranscriptEntry[] = [
  { role: "user", text: "试试看" },
  { role: "assistant", text: "我能听到你。" },
  { role: "user", text: "能进入 thread 吗？" },
];

function event(text = "能进入 thread 吗？"): AgentStreamEventPayload {
  return {
    type: "timeline",
    provider: "codex",
    turnId: "turn",
    item: {
      type: "user_message",
      messageId: "voice",
      text,
      voiceTranscript,
    },
  };
}

it.each(["能进入 thread 吗？", ""])(
  "keeps speech in live and replayed messages with input %j",
  (text) => {
    const live = applyStreamEvent({
      tail: [],
      head: [],
      event: event(text),
      timestamp: new Date(1),
    });
    const replay = hydrateStreamState([{ event: event(text), timestamp: new Date(1) }]);
    expect(live.tail).toEqual([expect.objectContaining({ text, voiceTranscript })]);
    expect(replay).toEqual([expect.objectContaining({ text, voiceTranscript })]);
  },
);

it("replaces a cached XML message when native history is refreshed without duplicating it", () => {
  const cached = createUserMessage({
    messageId: "voice",
    text: "<realtime_delegation>old</realtime_delegation>",
    timestamp: new Date(1),
  });
  const first = applyStreamEvent({
    tail: [cached],
    head: [],
    event: event(),
    timestamp: new Date(2),
  });
  const second = applyStreamEvent({
    tail: first.tail,
    head: first.head,
    event: event(),
    timestamp: new Date(2),
  });
  expect(second.tail).toEqual([
    expect.objectContaining({ text: "能进入 thread 吗？", voiceTranscript }),
  ]);
});

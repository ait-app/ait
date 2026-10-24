import {
  applyStreamEvent,
  hydrateStreamState,
  type StreamItem,
  type ThoughtItem,
  type UserMessageItem,
} from "@/types/stream";
import type { AgentStreamEventPayload } from "@ait/protocol/messages";
import { describe, expect, it } from "vitest";
import { buildAgentStreamRenderModel } from "./model";
import { createStreamPresentation, getStreamItemMessageId } from "./presentation";

const presentationOptions = { level: "overview" as const, isTurnActive: true };

function streamHarness() {
  let state: { tail: StreamItem[]; head: StreamItem[] } = { tail: [], head: [] };
  const present = createStreamPresentation();
  return {
    send(event: AgentStreamEventPayload) {
      state = applyStreamEvent({ ...state, event, timestamp: new Date(1000) });
      return this.render();
    },
    render() {
      return present({ ...presentationOptions, ...state });
    },
    source: () => state,
  };
}

function assistant(text: string, messageId = "message-1"): AgentStreamEventPayload {
  return {
    type: "timeline",
    provider: "claude",
    item: { type: "assistant_message", messageId, text },
  };
}

function rows(result: { tail: StreamItem[]; head: StreamItem[] }): StreamItem[] {
  return [...result.tail, ...result.head];
}

function thought(id: string, text: string, status: ThoughtItem["status"] = "ready"): ThoughtItem {
  return { kind: "thought", id, text, status, timestamp: new Date(1000) };
}

describe.each(["overview", "detailed"] as const)(
  "consecutive thoughts in %s presentation",
  (level) => {
    const options = { ...presentationOptions, level };

    it.each(["history", "live", "boundary"] as const)(
      "merges a %s run into its first entry without changing the source",
      (location) => {
        const first = Object.freeze({
          ...thought("first", "First thought"),
          timelineCursor: { epoch: "epoch", seq: 1 },
        });
        const second = Object.freeze(thought("second", "Second thought"));
        const last = Object.freeze({
          ...thought("last", "Latest thought", "loading"),
          timestamp: new Date(3000),
        });
        const tail =
          location === "history" ? [first, second, last] : location === "boundary" ? [first] : [];
        const head =
          location === "history"
            ? []
            : location === "boundary"
              ? [second, last]
              : [first, second, last];
        const present = createStreamPresentation();
        const projected = present({ ...options, tail, head });

        expect(rows(projected)).toEqual([
          {
            ...first,
            text: "First thought\n\nSecond thought\n\nLatest thought",
            status: "loading",
            timestamp: last.timestamp,
          },
        ]);
        expect(first.text).toBe("First thought");
        expect(rows(present({ ...options, tail, head }))[0]).toBe(rows(projected)[0]);
        for (const platform of ["web", "native"] as const) {
          const model = buildAgentStreamRenderModel({
            ...projected,
            isTurnActive: true,
            activeTurnStartedAt: first.timestamp,
            platform,
            isMobileBreakpoint: true,
          });
          expect([...model.history, ...model.segments.liveHead]).toEqual(rows(projected));
        }
      },
    );

    it("keeps one entry as a boundary run grows and is committed", () => {
      const first = thought("first", "First thought");
      const tail = [first];
      const present = createStreamPresentation();
      const live = thought("live", "Partial", "loading");
      const initial = present({ ...options, tail, head: [live] });
      const finished = { ...live, text: "Complete", status: "ready" as const };
      const growing = present({ ...options, tail, head: [finished] });
      const completed = present({
        ...options,
        tail: [first, finished],
        head: [],
        isTurnActive: false,
      });

      expect(initial.head).toEqual([]);
      expect(initial.tail).toMatchObject([
        { id: "first", text: "First thought\n\nPartial", status: "loading" },
      ]);
      expect(growing.tail).toMatchObject([
        { id: "first", text: "First thought\n\nComplete", status: "ready" },
      ]);
      expect(completed.tail).toEqual(growing.tail);
      expect(completed.tail[0]).toBe(growing.tail[0]);
      expect(completed.head).toEqual([]);
    });

    it("keeps thoughts separated by visible content or different turns", () => {
      const items: StreamItem[] = [
        thought("first", "First"),
        assistantMessage("answer", 2),
        thought("second", "Second"),
        {
          kind: "tool_call",
          id: "tool",
          timestamp: new Date(2000),
          payload: {
            source: "orchestrator",
            data: { toolCallId: "tool", toolName: "Shell", arguments: "pwd", status: "completed" },
          },
        },
        { ...thought("third", "Third"), turnId: "turn-1" },
        { ...thought("fourth", "Fourth"), turnId: "turn-2" },
        userMessage("user", 3),
        thought("fifth", "Fifth"),
      ];
      const projected = createStreamPresentation()({ ...options, tail: items, head: [] });
      expect(rows(projected).filter((item) => item.kind === "thought")).toEqual(
        items.filter((item) => item.kind === "thought"),
      );
    });

    it("reuses merged history while unrelated live content changes", () => {
      const tail = [thought("first", "First"), thought("second", "Second")];
      const present = createStreamPresentation();
      const first = present({ ...options, tail, head: [assistantMessage("answer", 2)] });
      const next = present({
        ...options,
        tail,
        head: [{ ...assistantMessage("answer", 3), text: "Growing answer" }],
      });
      expect(next.tail).toBe(first.tail);
      expect(next.tail[0]).toBe(first.tail[0]);
    });
  },
);

describe("native stream presentation", () => {
  it("keeps completed native blocks stable while the last block grows and finishes", () => {
    const harness = streamHarness();
    const first = harness.send(assistant("Intro\n\n![Image](image.png)"));
    const growing = harness.send(assistant("\n\nAfter\n"));
    const last = harness.send(assistant("- item"));
    const completed = harness.send({ type: "turn_completed", provider: "claude" });
    expect(first.tail).toMatchObject([{ text: "Intro", blockIndex: 0 }]);
    expect(first.head).toMatchObject([{ text: "![Image](image.png)", blockIndex: 1 }]);
    expect(growing.tail[0]).toBe(first.tail[0]);
    expect(growing.tail[1]).toBe(first.head[0]);
    expect(last.tail).toBe(growing.tail);
    expect(last.head).toMatchObject([{ text: "After\n- item", blockIndex: 2 }]);
    expect(completed.tail).toEqual(rows(last));
    expect(completed.tail[2]).toBe(last.head[0]);
    expect(completed.head).toEqual([]);
  });

  it("keeps blank lines inside an open native code fence", () => {
    const harness = streamHarness();
    harness.send(assistant("Intro\n\n```ts\nconst a = 1;"));
    const result = harness.send(assistant("\n\nconst b = 2;"));
    expect(result.tail).toMatchObject([{ text: "Intro" }]);
    expect(result.head).toMatchObject([{ text: "```ts\nconst a = 1;\n\nconst b = 2;" }]);
  });

  it("refreshes every retained block's cursor when the canonical message replaces live metadata", () => {
    const present = createStreamPresentation();
    const source = {
      ...assistantMessage("canonical", 1),
      text: "First block\n\nLast block",
      timelineCursor: { epoch: "old", seq: 1 },
    };
    const first = rows(present({ ...presentationOptions, tail: [], head: [source] }));
    const canonical = {
      ...source,
      text: source.text + " grows",
      timestamp: createTimestamp(2),
      timelineCursor: { epoch: "new", seq: 5 },
    };
    const next = rows(present({ ...presentationOptions, tail: [canonical], head: [] }));
    expect(next.map((item) => item.id)).toEqual(first.map((item) => item.id));
    expect(
      next.every((item) => item.timelineCursor?.epoch === "new" && item.timelineCursor.seq === 5),
    ).toBe(true);
    expect(next.every((item) => item.timestamp === canonical.timestamp)).toBe(true);
    expect(next[0]).not.toBe(first[0]);
  });

  it("preserves newlines when code indentation arrives in separate deltas", () => {
    const harness = streamHarness();
    harness.send(assistant("Intro\n\n```text\ndomain::agent_runtime::PersistedAgentRuntimeRecord"));
    harness.send(assistant("\n"));
    harness.send(assistant("   "));
    harness.send(assistant(" 定义保存的数据\n"));
    harness.send(assistant("                 "));
    const result = harness.send(
      assistant(" ↓\nprovider::storage::agent_runtime::FileBackedAgentRuntimeRegistry\n```"),
    );

    expect(result.head).toMatchObject([
      {
        text: "```text\ndomain::agent_runtime::PersistedAgentRuntimeRecord\n    定义保存的数据\n                  ↓\nprovider::storage::agent_runtime::FileBackedAgentRuntimeRegistry\n```",
      },
    ]);
    expect(harness.send({ type: "turn_completed", provider: "claude" }).tail).toEqual(rows(result));
  });

  it.each([
    "```text\nfirst\n    second\n\n  \n\tthird\n```",
    "```text\nfirst\n  \n \n```",
    '```mermaid\nflowchart TD\n    D --> E["Daemon::set_config"]\n    E --> F["DaemonConfigStore::patch"]\n```',
    "first\n    second\nthird",
    "- first\n  continuation\n\n- second",
  ])("keeps the growing block source intact at every character boundary: %s", (block) => {
    const harness = streamHarness();
    harness.send(assistant("Intro\n\n"));
    for (let length = 1; length <= block.length; length++) {
      const result = harness.send(assistant(block[length - 1]));
      expect(result.head).toMatchObject([{ text: block.slice(0, length) }]);
    }
  });

  it("leaves fetched native Markdown intact, including cross-paragraph references", () => {
    const source = hydrateStreamState([
      {
        event: assistant("[Link][docs]\n\n[docs]: https://example.com"),
        timestamp: new Date(1000),
      },
    ]);
    const result = createStreamPresentation()({
      ...presentationOptions,
      tail: source,
      head: [],
    });
    expect(result.tail).toMatchObject([
      { text: "[Link][docs]\n\n[docs]: https://example.com", blockGroupId: source[0]!.id },
    ]);
    expect(getStreamItemMessageId(result.tail[0]!)).toBe(source[0]!.id);
    expect(source[0]!.id).toBe("message-1");
  });
});

function createTimestamp(seed: number): Date {
  return new Date(`2026-01-01T00:00:${seed.toString().padStart(2, "0")}.000Z`);
}

function userMessage(id: string, seed: number): UserMessageItem {
  return {
    kind: "user_message",
    id,
    text: id,
    timestamp: createTimestamp(seed),
  };
}

function assistantMessage(
  id: string,
  seed: number,
): Extract<StreamItem, { kind: "assistant_message" }> {
  return {
    kind: "assistant_message",
    id,
    text: id,
    timestamp: createTimestamp(seed),
  };
}

describe("timeline presentation", () => {
  const present = createStreamPresentation();
  function projectTimelineItems(items: StreamItem[]) {
    return present({ ...presentationOptions, tail: items, head: [] }).tail;
  }
  const envelope =
    "<spoken-input>\nPlease fix the voice chat.\n</spoken-input>\n<instruction>This message was spoken by the user. Respond using the speak tool only, not normal messages, because the user may not be looking at the chat.</instruction>";

  it.each(["live", "history"])(
    "shows only spoken words from a %s user message without mutating its source",
    (source) => {
      const item: UserMessageItem = Object.freeze({
        ...userMessage(source, 1),
        text: envelope,
        messageId: "provider-message",
        clientMessageId: "client-message",
        turnId: "turn-1",
        timelineCursor: { epoch: "epoch", seq: 12 },
      });
      const presentMessage = () =>
        present({
          ...presentationOptions,
          tail: source === "history" ? [item] : [],
          head: source === "live" ? [item] : [],
        });
      const projected = rows(presentMessage());
      expect(projected).toEqual([{ ...item, text: "Please fix the voice chat." }]);
      const model = buildAgentStreamRenderModel({
        tail: source === "history" ? projected : [],
        head: source === "live" ? projected : [],
        isTurnActive: source === "live",
        activeTurnStartedAt: item.timestamp,
        platform: "native",
        isMobileBreakpoint: true,
      });
      const rendered = [...model.history, ...model.segments.liveHead];
      expect(rendered).toContainEqual({ ...item, text: "Please fix the voice chat." });
      expect(item.text).toBe(envelope);
      expect(rows(presentMessage())[0]).toBe(projected[0]);
    },
  );

  it("supports older envelopes and preserves multiline spoken content", () => {
    const item = {
      ...userMessage("legacy", 1),
      text: "<spoken-input>\nFirst line.\nSecond line with <example>XML</example>.\n</spoken-input>",
    };
    expect(projectTimelineItems([item])).toEqual([
      { ...item, text: "First line.\nSecond line with <example>XML</example>." },
    ]);
  });

  it("leaves ordinary messages, assistant examples and incomplete wrappers unchanged", () => {
    const items: StreamItem[] = [
      userMessage("ordinary", 1),
      { ...assistantMessage("example", 2), text: envelope },
      { ...userMessage("incomplete", 3), text: "<spoken-input>unfinished" },
      { ...userMessage("quoted", 4), text: "Explain this example: " + envelope },
      {
        ...userMessage("xml", 5),
        text: "<spoken-input>Example</spoken-input><instruction>Explain this XML.</instruction>",
      },
    ];
    const projected = projectTimelineItems(items);
    expect(projected.map((item) => ("text" in item ? item.text : null))).toEqual(
      items.map((item) => ("text" in item ? item.text : null)),
    );
    expect(projected.map(getStreamItemMessageId)).toEqual(items.map((item) => item.id));
    expect(projectTimelineItems(items)[0]).toBe(items[0]);
  });
});

import { describe, expect, it } from "vitest";
import type { StreamItem, ThoughtItem } from "@/types/stream";
import { createTurnThinkingProjection } from "./thinking-by-turn";

function thought(
  id: string,
  turnId?: string,
  status: ThoughtItem["status"] = "ready",
): ThoughtItem {
  return { kind: "thought", id, turnId, text: id, timestamp: new Date(1000), status };
}

function reply(id: string, turnId?: string): StreamItem {
  return { kind: "assistant_message", id, turnId, text: id, timestamp: new Date(1000) };
}

function thoughts(result: { tail: StreamItem[]; head: StreamItem[] }) {
  return [...result.tail, ...result.head].filter((item) => item.kind === "thought");
}

describe("turn thinking projection", () => {
  it("groups a turn across history and live rows while retaining source order and metadata", () => {
    const first = {
      ...thought("first", "turn-1"),
      timelineCursor: { epoch: "epoch", seq: 1 },
    };
    const between = reply("between", "turn-1");
    const tail = [first, between, thought("second", "turn-1")];
    const head = [thought("third", "turn-1", "loading"), reply("answer", "turn-1")];
    const result = createTurnThinkingProjection()({ tail, head });

    expect(thoughts(result)).toEqual([
      {
        ...first,
        id: "thinking:turn:turn-1",
        text: "first\n\nsecond\n\nthird",
        status: "loading",
      },
    ]);
    expect(result.tail[1]).toBe(between);
    expect(result.head).toEqual([head[1]]);
    expect(tail.map((item) => item.id)).toEqual(["first", "between", "second"]);
    expect(head).toHaveLength(2);
    expect(first.status).toBe("ready");
  });

  it("keeps different turns and legacy reasoning separate, including interleaved turns", () => {
    const legacy = thought("legacy");
    const tail = [thought("a1", "a"), thought("b1", "b"), legacy, thought("a2", "a")];
    const result = createTurnThinkingProjection()({
      tail,
      head: [thought("b2", "b"), thought("c", "c"), thought("legacy-live")],
    });

    expect(thoughts(result)).toMatchObject([
      { id: "thinking:turn:a", text: "a1\n\na2" },
      { id: "thinking:turn:b", text: "b1\n\nb2" },
      { id: "legacy", text: "legacy" },
      { id: "thinking:turn:c", text: "c" },
      { id: "legacy-live", text: "legacy-live" },
    ]);
    expect(result.tail[2]).toBe(legacy);
  });

  it("retains the turn row id across streaming, settlement, reload and older history", () => {
    const project = createTurnThinkingProjection();
    const first = thought("live", "turn:/1", "loading");
    const initial = thoughts(project({ tail: [], head: [first] }))[0]!;
    const settled = thought("canonical", "turn:/1");
    const completed = thoughts(project({ tail: [settled], head: [] }))[0]!;
    const restored = thoughts(createTurnThinkingProjection()({ tail: [settled], head: [] }))[0]!;
    const older = thoughts(project({ tail: [thought("older", "turn:/1"), settled], head: [] }))[0]!;

    expect(initial.id).toBe("thinking:turn:turn%3A%2F1");
    expect([completed.id, restored.id, older.id]).toEqual([initial.id, initial.id, initial.id]);
    expect(completed.status).toBe("ready");
    expect(older.text).toBe("older\n\ncanonical");
  });

  it("reuses unaffected history rows and caches unchanged input", () => {
    const project = createTurnThinkingProjection();
    const tail = [thought("old", "old-turn"), reply("old answer"), thought("start", "turn")];
    const source = thought("live", "turn", "loading");
    const head = [source];
    const initial = project({ tail, head });
    const same = project({ tail, head });
    const next = project({ tail, head: [{ ...source, text: "live grows" }] });

    expect(same).toBe(initial);
    expect(next.tail[0]).toBe(initial.tail[0]);
    expect(next.tail[1]).toBe(initial.tail[1]);
    expect(next.tail[2]).toMatchObject({ id: initial.tail[2]!.id, text: "start\n\nlive grows" });
    expect(next.head).toEqual([]);
  });

  it("preserves array identity when no turn reasoning needs projection", () => {
    const tail = [reply("answer"), thought("legacy")];
    const head = [reply("live answer")];
    const result = createTurnThinkingProjection()({ tail, head });
    expect(result.tail).toBe(tail);
    expect(result.head).toBe(head);
  });

  it.each([0, 1, 2, 3, 4])(
    "keeps legacy rows separated by a coalesced canonical turn with %i history rows",
    (historyLength) => {
      const items = [
        thought("canonical first", "turn"),
        thought("legacy first"),
        thought("canonical second", "turn"),
        thought("legacy second"),
      ];
      const result = createTurnThinkingProjection()({
        tail: items.slice(0, historyLength),
        head: items.slice(historyLength),
      });

      expect(thoughts(result)).toMatchObject([
        { id: "thinking:turn:turn", text: "canonical first\n\ncanonical second" },
        { id: "legacy first", text: "legacy first" },
        { id: "legacy second", text: "legacy second" },
      ]);
    },
  );
});

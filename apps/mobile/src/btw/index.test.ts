import { describe, expect, it, vi } from "vitest";
import { dispatchBtwCommand, routeBtwCommand, resolveBtwCommand, btwTopicTurns } from "./index";
import type { SideTopic } from "@ait/protocol/messages";

function topic(overrides: Partial<SideTopic> = {}): SideTopic {
  return {
    id: "topic",
    question: "First question",
    answer: "First answer",
    status: "complete",
    createdAt: 1,
    updatedAt: 2,
    leafId: null,
    strategy: "native",
    truncated: false,
    ...overrides,
  };
}

describe("resolveBtwCommand", () => {
  it("accepts a bare /btw as history and /btw <question> as an ask", () => {
    expect(resolveBtwCommand({ text: "/btw", hasAttachments: false })).toEqual({ question: "" });
    expect(resolveBtwCommand({ text: " /btw: what changed? ", hasAttachments: false })).toEqual({
      question: "what changed?",
    });
  });

  it("leaves paths and other commands on the normal submit path", () => {
    expect(resolveBtwCommand({ text: "/btw/file.ts", hasAttachments: false })).toBeNull();
    expect(resolveBtwCommand({ text: "/btwx", hasAttachments: false })).toBeNull();
    expect(resolveBtwCommand({ text: "hello", hasAttachments: false })).toBeNull();
  });

  it("keeps a BTW command with attachments on the side-question route for explicit rejection", () => {
    expect(resolveBtwCommand({ text: "/btw question", hasAttachments: true })).toEqual({
      question: "question",
      attachmentsUnsupported: true,
    });
  });
});

describe("BTW command ownership", () => {
  it("preserves legacy and unclaimed side questions while respecting native names and aliases", () => {
    expect(routeBtwCommand({ kind: "legacy" })).toBe("side");
    expect(routeBtwCommand({ kind: "unavailable" })).toBe("uncertain");
    expect(routeBtwCommand({ kind: "complete", commands: [] })).toBe("side");
    for (const commands of [
      [{ name: "btw", source: "extension" }],
      [{ name: "btw", source: "builtin" }],
      [{ name: "review", aliases: ["btw:"], source: "skill" }],
    ])
      expect(routeBtwCommand({ kind: "complete", commands })).toBe("native");
    for (const commands of [
      [{ name: "btw" }],
      [
        { name: "btw", source: "builtin" },
        { name: "btw", source: "extension" },
      ],
      [{ name: "btw", source: "unknown" }],
    ])
      expect(routeBtwCommand({ kind: "complete", commands })).toBe("uncertain");
  });

  it("dispatches only after fresh ownership is known", async () => {
    const onSide = vi.fn();
    const onNative = vi.fn();
    const onUncertain = vi.fn();
    const run = (ownership: Parameters<typeof routeBtwCommand>[0] | null) =>
      dispatchBtwCommand({
        checkOwnership: async () => ownership,
        onSide,
        onNative,
        onUncertain,
      });
    await run({
      kind: "complete",
      commands: [{ name: "other", aliases: ["btw"], source: "extension" }],
    });
    expect(onNative).toHaveBeenCalledTimes(1);
    expect(onSide).not.toHaveBeenCalled(); // No side-question history is needed for a native command.
    await run({ kind: "complete", commands: [{ name: "btw" }] });
    expect(onUncertain).toHaveBeenCalledTimes(1);
    await run(null);
    expect(onSide).not.toHaveBeenCalled();
    await run({ kind: "complete", commands: [] });
    await run({ kind: "legacy" });
    expect(onSide).toHaveBeenCalledTimes(2);
  });
});

describe("btwTopicTurns", () => {
  it("orders the root turn before follow-ups so callers read the latest status last", () => {
    const turns = btwTopicTurns(
      topic({
        followUps: [
          {
            question: "Second question",
            answer: "Second answer",
            status: "running",
            createdAt: 3,
            updatedAt: 4,
          },
        ],
      }),
    );
    expect(turns.map((turn) => [turn.question, turn.status])).toEqual([
      ["First question", "complete"],
      ["Second question", "running"],
    ]);
  });
});

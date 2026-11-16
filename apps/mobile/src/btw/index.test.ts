import { describe, expect, it } from "vitest";
import { resolveBtwCommand, btwTopicTurns } from "./index";
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

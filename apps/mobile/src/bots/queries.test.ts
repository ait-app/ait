import { describe, expect, it, vi } from "vitest";
import type { BotTurn } from "@ait/protocol/bot/types";
import { fetchBotHistory } from "./queries";

vi.mock("@/runtime/host-runtime", () => ({ useHostRuntimeClient: vi.fn() }));

function turn(sequence: number, status: BotTurn["status"]): BotTurn {
  return {
    id: `turn-${sequence}`,
    conversationId: "conversation",
    messageId: `m-${sequence}`,
    sequence,
    generation: 1,
    text: `input ${sequence}`,
    replyTo: null,
    reply: status === "completed" ? "reply" : null,
    pendingPermissions: [],
    status,
    delivery: "pending",
    error: null,
    createdAt: "2026-10-12",
  };
}

describe("Bot receipt history", () => {
  it("refreshes active work and reads new input without fetching completed history again", async () => {
    const botRequest = vi
      .fn()
      .mockImplementation(async (method) =>
        method === "bot.turn.get.request"
          ? { turn: turn(2, "completed") }
          : { turns: [turn(3, "queued")], nextSequence: 3 },
      );
    const result = await fetchBotHistory({ botRequest }, "bot", "conversation", [
      turn(1, "completed"),
      turn(2, "running"),
    ]);
    expect(result.map((item) => [item.sequence, item.status])).toEqual([
      [1, "completed"],
      [2, "completed"],
      [3, "queued"],
    ]);
    expect(botRequest).toHaveBeenCalledWith("bot.turn.get.request", {
      botId: "bot",
      turnId: "turn-2",
    });
    expect(botRequest).toHaveBeenCalledWith("bot.history.get.request", {
      botId: "bot",
      conversationId: "conversation",
      afterSequence: 2,
      limit: 50,
    });
    expect(botRequest).toHaveBeenCalledTimes(2);
  });
  it("restores history across server pages and stops at the final page", async () => {
    const botRequest = vi
      .fn()
      .mockResolvedValueOnce({
        turns: Array.from({ length: 50 }, (_, index) => turn(index + 1, "completed")),
        nextSequence: 50,
      })
      .mockResolvedValueOnce({ turns: [turn(51, "completed")], nextSequence: 51 });
    expect(await fetchBotHistory({ botRequest }, "bot", "conversation")).toHaveLength(51);
    expect(botRequest.mock.calls[1][1]).toMatchObject({ afterSequence: 50 });
    expect(botRequest).toHaveBeenCalledTimes(2);
  });
});

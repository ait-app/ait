import { describe, expect, it } from "vitest";
import { BOT_METHODS, BOT_REQUEST_SCHEMAS, BotRequestSchema } from "./rpc-schemas.js";
import { SessionInboundMessageSchema, SessionOutboundMessageSchema } from "../messages.js";

describe("Bot wire contract", () => {
  it("registers all 21 methods and accepts their results in the shared envelope", () => {
    expect(new Set(BOT_METHODS).size).toBe(21);
    expect(BOT_REQUEST_SCHEMAS).toHaveLength(21);
    expect(
      SessionInboundMessageSchema.parse({
        type: "bot.conversation.open.request",
        requestId: "r1",
        botId: "11111111-1111-4111-8111-111111111111",
      }).type,
    ).toBe("bot.conversation.open.request");
    expect(
      SessionOutboundMessageSchema.parse({
        type: "bot.list.response",
        payload: { requestId: "r1", bots: [] },
      }).type,
    ).toBe("bot.list.response");
  });
  it("rejects malformed identity, unknown fields and invalid history page sizes", () => {
    const valid = {
      type: "bot.history.get.request",
      requestId: "r1",
      botId: "11111111-1111-4111-8111-111111111111",
      conversationId: "22222222-2222-4222-8222-222222222222",
    };
    expect(BotRequestSchema.safeParse(valid).success).toBe(true);
    for (const patch of [
      { botId: "bad" },
      { limit: 51 },
      { limit: 0 },
      { afterSequence: -1 },
      { token: "must-not-be-accepted" },
    ]) {
      expect(BotRequestSchema.safeParse({ ...valid, ...patch }).success).toBe(false);
    }
  });
});

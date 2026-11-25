import { expect, it } from "vitest";
import { BotRequestSchema, BotResponseSchema, BOT_METHODS } from "./rpc-schemas.js";

it("addresses chat reception by channel and peer and resets context by workspace", () => {
  const request = {
    type: "bot.chat.update.request",
    requestId: "r",
    workspaceId: "w",
    channelId: "00000000-0000-4000-8000-000000000001",
    peer: "oc_a",
    enabled: true,
    listenAllMessages: true,
  };
  expect(BotRequestSchema.parse(request)).toEqual(request);
  expect(() => BotRequestSchema.parse({ ...request, sender: "ou_a" })).toThrow();
  expect(
    BotRequestSchema.parse({ type: "bot.context.reset.request", requestId: "r", workspaceId: "w" }),
  ).toHaveProperty("workspaceId", "w");
  expect(BOT_METHODS).not.toContain("bot.conversation.reset.request");
  expect(
    BotResponseSchema.parse({
      type: "bot.chat.list.response",
      payload: {
        requestId: "r",
        chats: [
          {
            channelId: request.channelId,
            peer: "oc_a",
            group: true,
            enabled: true,
            listenAllMessages: true,
          },
        ],
      },
    }),
  ).toHaveProperty("payload.chats.0.peer", "oc_a");
});

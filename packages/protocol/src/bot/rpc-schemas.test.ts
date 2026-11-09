import { describe, expect, it } from "vitest";
import {
  BOT_METHODS,
  BOT_REQUEST_SCHEMAS,
  BotRequestSchema,
  BotResponseSchema,
} from "./rpc-schemas.js";
import { BotPermissionsSchema } from "./types.js";
import { SessionInboundMessageSchema, SessionOutboundMessageSchema } from "../messages.js";

describe("Bot wire contract", () => {
  it("parses permission profiles and the update method", () => {
    const parsed = BotRequestSchema.parse({
      type: "bot.permissions.update.request",
      requestId: "r",
      workspaceId: "wks_1",
      permissions: { groups: ["agents"], modes: { codex: ["auto"] } },
    });
    expect(parsed.type).toBe("bot.permissions.update.request");
    expect(() => BotPermissionsSchema.parse({ groups: ["root"], modes: {} })).toThrow();
    expect(() => BotPermissionsSchema.parse({ groups: [], modes: { codex: [] } })).toThrow();
    expect(BOT_METHODS).not.toContain("bot.permissions.update.request");
    expect(
      BotResponseSchema.parse({
        type: "bot.permissions.update.response",
        payload: {
          requestId: "r",
          appliesToNextTurn: true,
          bot: {
            workspaceId: "wks_1",
            execution: { provider: "codex", model: null, modeId: null, thinking: null },
            builtinChannelId: crypto.randomUUID(),
            state: { kind: "active" },
            permissions: { groups: ["observe"], modes: {} },
            createdAt: "2026-01-01T00:00:00Z",
          },
        },
      }).type,
    ).toBe("bot.permissions.update.response");
  });

  it("registers all 21 core methods and accepts their results in the shared envelope", () => {
    expect(new Set(BOT_METHODS).size).toBe(21);
    expect(BOT_REQUEST_SCHEMAS).toHaveLength(26);
    expect(
      SessionInboundMessageSchema.parse({
        type: "bot.conversation.open.request",
        requestId: "r1",
        workspaceId: "wks_1",
      }).type,
    ).toBe("bot.conversation.open.request");
    expect(
      SessionOutboundMessageSchema.parse({
        type: "bot.get.response",
        payload: { requestId: "r1", bot: null },
      }).type,
    ).toBe("bot.get.response");
  });
  it("parses v2 workspace-addressed requests and rejects removed v1 methods", () => {
    expect(
      BotRequestSchema.parse({
        type: "bot.setup.request",
        requestId: "r1",
        workspaceId: "wks_1",
        execution: { provider: "codex", model: null, modeId: null, thinking: null },
        clearMemory: true,
      }).type,
    ).toBe("bot.setup.request");
    expect(() =>
      BotRequestSchema.parse({ type: "bot.list.request", requestId: "r", workspaceId: "w" }),
    ).toThrow();
    expect(() =>
      BotRequestSchema.parse({
        type: "bot.get.request",
        requestId: "r",
        botId: crypto.randomUUID(),
      }),
    ).toThrow();
  });
  it("parses unconfigured and paused bot views", () => {
    const unconfigured = BotResponseSchema.parse({
      type: "bot.get.response",
      payload: { requestId: "r", bot: null },
    });
    expect(unconfigured.payload).toMatchObject({ bot: null });
    const paused = BotResponseSchema.parse({
      type: "bot.pause.response",
      payload: {
        requestId: "r",
        bot: {
          workspaceId: "wks_1",
          execution: { provider: "codex", model: null, modeId: null, thinking: null },
          builtinChannelId: crypto.randomUUID(),
          state: { kind: "paused", reason: "directory_missing" },
          createdAt: "2026-11-05T00:00:00Z",
        },
      },
    });
    expect(paused.type).toBe("bot.pause.response");
  });
  it("rejects malformed identity, unknown fields and invalid history page sizes", () => {
    const valid = {
      type: "bot.history.get.request",
      requestId: "r1",
      workspaceId: "wks_1",
      conversationId: "22222222-2222-4222-8222-222222222222",
    };
    expect(BotRequestSchema.safeParse(valid).success).toBe(true);
    for (const patch of [
      { workspaceId: " " },
      { limit: 51 },
      { limit: 0 },
      { afterSequence: -1 },
      { token: "must-not-be-accepted" },
    ]) {
      expect(BotRequestSchema.safeParse({ ...valid, ...patch }).success).toBe(false);
    }
  });
});

import { describe, expect, it } from "vitest";
import {
  BOT_METHODS,
  BOT_REQUEST_SCHEMAS,
  BotRequestSchema,
  BotResponseSchema,
} from "./rpc-schemas.js";
import { BotPermissionsSchema, BotTurnSchema } from "./types.js";
import { SessionInboundMessageSchema, SessionOutboundMessageSchema } from "../messages.js";

describe("Bot wire contract", () => {
  it("preserves distinct assistant messages and defaults legacy turns to an empty list", () => {
    const turn = {
      id: crypto.randomUUID(),
      conversationId: crypto.randomUUID(),
      messageId: "input",
      sequence: 1,
      generation: 0,
      text: "question",
      replyTo: null,
      reply: "second",
      pendingPermissions: [],
      status: "completed",
      delivery: "delivered",
      error: null,
      createdAt: "2026-11-10T00:00:00Z",
    } as const;
    expect(
      BotTurnSchema.parse({
        ...turn,
        messages: [
          { id: "first", text: "first", status: "completed" },
          { id: "second", text: "second", status: "completed" },
        ],
      }).messages.map((message) => message.id),
    ).toEqual(["first", "second"]);
    expect(BotTurnSchema.parse(turn).messages).toEqual([]);
  });

  it("keeps WeChat and QQ setup optional and parses their channels", () => {
    expect(BOT_METHODS).not.toContain("bot.channel.weixin.scan.start.request");
    expect(BOT_METHODS).not.toContain("bot.channel.qq.pair.request");
    const scan = BotResponseSchema.parse({
      type: "bot.channel.qq.scan.status.response",
      payload: { requestId: "r", state: "failed", reason: "alreadyConnected" },
    });
    expect(scan.type).toBe("bot.channel.qq.scan.status.response");
    const channel = (config: unknown) => ({
      type: "bot.channel.weixin.scan.status.response",
      payload: {
        requestId: "r",
        state: "complete",
        channel: {
          id: crypto.randomUUID(),
          config,
          enabled: true,
          cursor: null,
          deleted: false,
        },
      },
    });
    const weixin = {
      kind: "weixin",
      accountId: "bot@im.bot",
      baseUrl: "https://ilinkai.weixin.qq.com",
      tokenRef: "env:AIT_SERVER_CREDENTIAL_STORED_X",
      allowedSenders: ["alice@im.wechat"],
    };
    expect(BotResponseSchema.parse(channel(weixin)).type).toBe(
      "bot.channel.weixin.scan.status.response",
    );
    expect(() => BotResponseSchema.parse(channel({ ...weixin, allowGroups: true }))).toThrow();
    expect(
      BotResponseSchema.parse(
        channel({ kind: "qq", appId: "1", secretRef: "env:X", allowedSenders: [] }),
      ).type,
    ).toBe("bot.channel.weixin.scan.status.response");
  });

  it("keeps iMessage setup optional and parses its pairing and channel", () => {
    expect(BOT_METHODS).not.toContain("bot.channel.imessage.pair.start.request");
    const start = BotRequestSchema.parse({
      type: "bot.channel.imessage.pair.start.request",
      requestId: "r",
      workspaceId: "w",
      projectId: "project",
      apiKey: "pho_sk_test",
      handle: "+14155550123",
    });
    expect(start.type).toBe("bot.channel.imessage.pair.start.request");
    const waiting = BotResponseSchema.parse({
      type: "bot.channel.imessage.pair.start.response",
      payload: {
        requestId: "r",
        state: "waiting",
        pairingId: crypto.randomUUID(),
        phoneNumber: "+14155550100",
        code: "ait-abcd",
        url: "sms:+14155550100&body=ait-abcd",
        expiresInSeconds: 600,
      },
    });
    expect(waiting.type).toBe("bot.channel.imessage.pair.start.response");
    for (const reason of ["invalidKey", "invalidHandle", "dedicatedLine"]) {
      expect(
        BotResponseSchema.parse({
          type: "bot.channel.imessage.pair.status.response",
          payload: { requestId: "r", state: "failed", reason },
        }).type,
      ).toBe("bot.channel.imessage.pair.status.response");
    }
    const channel = (config: unknown) => ({
      type: "bot.channel.imessage.pair.status.response",
      payload: {
        requestId: "r",
        state: "complete",
        channel: {
          id: crypto.randomUUID(),
          config,
          enabled: true,
          cursor: null,
          deleted: false,
        },
      },
    });
    const config = {
      kind: "photon_imessage",
      projectId: "project",
      keyRef: "env:AIT_SERVER_CREDENTIAL_STORED_X",
      allowedSenders: ["+14155550123"],
    };
    expect(BotResponseSchema.parse(channel(config)).type).toBe(
      "bot.channel.imessage.pair.status.response",
    );
    expect(() => BotResponseSchema.parse(channel({ ...config, allowGroups: true }))).toThrow();
  });

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
    expect(BOT_REQUEST_SCHEMAS).toHaveLength(40);
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

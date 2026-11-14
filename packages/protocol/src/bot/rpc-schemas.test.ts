import { describe, expect, it } from "vitest";
import {
  BOT_METHODS,
  BOT_NOTIFICATION_METHODS,
  BOT_REQUEST_SCHEMAS,
  BOT_TICKET_METHODS,
  BotRequestSchema,
  BotResponseSchema,
} from "./rpc-schemas.js";
import {
  BotChannelConfigSchema,
  BotConversationSchema,
  BotNotificationSchema,
  BotPermissionsSchema,
  BotSkipReasonSchema,
  BotTicketSummarySchema,
  BotTurnSchema,
  BotWakeStatusSchema,
} from "./types.js";
import { SessionInboundMessageSchema, SessionOutboundMessageSchema } from "../messages.js";
import { StoredScheduleSchema } from "../schedule/types.js";
import { validateWSOutboundMessage } from "../validation/ws-outbound.js";

describe("Bot wire contract", () => {
  it("keeps Lark setup optional and accepts its credentials and channel in session envelopes", () => {
    expect(BOT_METHODS).not.toContain("bot.channel.lark.connect.request");
    const channelId = crypto.randomUUID();
    expect(
      SessionInboundMessageSchema.parse({
        type: "bot.channel.lark.connect.request",
        requestId: "lark-connect",
        workspaceId: "workspace",
        channelId,
        appId: "cli_lark",
        appSecret: "test-secret",
        allowedSenders: ["ou_alice"],
      }).type,
    ).toBe("bot.channel.lark.connect.request");
    const config = {
      kind: "lark",
      appId: "cli_lark",
      secretRef: "env:AIT_SERVER_CREDENTIAL_LARK",
      allowedSenders: ["ou_alice"],
    };
    expect(BotChannelConfigSchema.parse(config)).toMatchObject({
      kind: "lark",
      allowGroups: false,
    });
    expect(
      SessionOutboundMessageSchema.parse({
        type: "bot.channel.lark.connect.response",
        payload: {
          requestId: "lark-connect",
          created: true,
          channel: { id: channelId, config, enabled: true, cursor: null },
        },
      }).type,
    ).toBe("bot.channel.lark.connect.response");
    expect(BotChannelConfigSchema.safeParse({ ...config, appSecret: "raw-secret" }).success).toBe(
      false,
    );
  });

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

  it("reads each conversation's channel kind and last activity, and older hosts without them", () => {
    const base = {
      id: crypto.randomUUID(),
      channelId: crypto.randomUUID(),
      peer: "chat-7",
      sender: "alice",
      thread: null,
      generation: 0,
      nextSequence: 2,
      sessionAgentId: "session",
    };
    expect(
      BotConversationSchema.parse({
        ...base,
        channelKind: "telegram",
        lastActivityAt: "2026-11-14T00:00:00Z",
      }),
    ).toMatchObject({ channelKind: "telegram", lastActivityAt: "2026-11-14T00:00:00Z" });
    expect(
      BotConversationSchema.parse({ ...base, lastActivityAt: null }).lastActivityAt,
    ).toBeNull();
    expect(BotConversationSchema.parse(base).channelKind).toBeUndefined();
  });

  it("registers all 21 core methods and accepts their results in the shared envelope", () => {
    expect(new Set(BOT_METHODS).size).toBe(21);
    expect(BOT_REQUEST_SCHEMAS).toHaveLength(47);
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
  it("parses a webhook channel and its one-time secret", () => {
    expect(BotChannelConfigSchema.safeParse({ kind: "hub" }).success).toBe(false);
    const channelId = crypto.randomUUID();
    const created = BotResponseSchema.parse({
      type: "bot.channel.webhook.create.response",
      payload: {
        requestId: "r",
        state: "created",
        secret: "s3cret",
        channel: {
          id: channelId,
          config: {
            kind: "webhook",
            endpointId: "0193f2c4-7d1e-7a40-9c1b-5e2f8a6d3b11",
            url: "https://aitapp.cn/api/v1/automation/ingress/webhook/0193f2c4",
          },
          enabled: true,
          cursor: null,
        },
      },
    });
    expect(created.payload).toMatchObject({ secret: "s3cret", channel: { id: channelId } });
    expect(
      BotResponseSchema.parse({
        type: "bot.channel.webhook.rotate.response",
        payload: { requestId: "r", state: "failed", reason: "centerOffline" },
      }).payload,
    ).toMatchObject({ reason: "centerOffline" });
    expect(() =>
      BotRequestSchema.parse({
        type: "bot.channel.webhook.rotate.request",
        requestId: "r",
        workspaceId: "wks_1",
        channelId: "not-a-uuid",
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

  it("parses wake turns and keeps turns from older hosts unchanged", () => {
    const base = {
      id: crypto.randomUUID(),
      conversationId: crypto.randomUUID(),
      messageId: "input",
      sequence: 1,
      generation: 0,
      text: "question",
      replyTo: null,
      reply: null,
      messages: [],
      pendingPermissions: [],
      status: "completed",
      delivery: "delivered",
      error: null,
      createdAt: "2026-11-12T00:00:00Z",
    } as const;
    const legacy = BotTurnSchema.parse(base);
    expect(legacy).toEqual(base);
    expect("wake" in legacy || "silent" in legacy || "notificationCursor" in legacy).toBe(false);
    const notification = {
      id: "agent:a1:2026-11-12T00:00:00Z",
      workspaceId: "wks_1",
      scope: "wks_1",
      kind: "agent_finished",
      agentId: "a1",
      scheduleId: null,
      runId: null,
      terminalId: null,
      title: 'Agent "fix" finished',
      summary: null,
      at: "2026-11-12T00:00:00Z",
      conversationId: base.conversationId,
    };
    const wake = BotTurnSchema.parse({
      ...base,
      messageId: `notification:${base.id}`,
      text: "<ait-notifications>…</ait-notifications>",
      wake: { notifications: [notification, { ...notification, id: "x", kind: "later_kind" }] },
      silent: true,
      notificationCursor: 7,
    });
    expect(wake.wake?.notifications.map((item) => item.kind)).toEqual([
      "agent_finished",
      "later_kind",
    ]);
    expect(wake).toMatchObject({ silent: true, notificationCursor: 7 });
  });

  it("keeps the notification log optional and parses wake scope and log entries", () => {
    expect(BOT_METHODS).not.toContain("bot.notifications.list.request");
    expect(BOT_NOTIFICATION_METHODS).toEqual(["bot.notifications.list.request"]);
    const execution = { provider: "codex", model: null, modeId: null, thinking: null };
    for (const type of ["bot.setup.request", "bot.update.request"] as const) {
      expect(
        BotRequestSchema.parse({
          type,
          requestId: "r",
          workspaceId: "w",
          execution,
          wakeScope: "all",
        }),
      ).toMatchObject({ wakeScope: "all" });
      expect(
        BotRequestSchema.safeParse({
          type,
          requestId: "r",
          workspaceId: "w",
          execution,
          wakeScope: "everything",
        }).success,
      ).toBe(false);
    }
    const list = {
      type: "bot.notifications.list.request",
      requestId: "r",
      workspaceId: "w",
    };
    expect(BotRequestSchema.safeParse(list).success).toBe(true);
    for (const patch of [{ limit: 0 }, { limit: 129 }, { unseenBy: "owner" }]) {
      expect(BotRequestSchema.safeParse({ ...list, ...patch }).success).toBe(false);
    }
    const bot = {
      workspaceId: "w",
      execution,
      builtinChannelId: crypto.randomUUID(),
      state: { kind: "active" },
      createdAt: "2026-11-12T00:00:00Z",
    };
    const view = (payload: unknown) =>
      BotResponseSchema.parse({
        type: "bot.get.response",
        payload: { requestId: "r", bot: payload },
      }).payload;
    expect(view(bot)).toEqual({ requestId: "r", bot });
    expect(view({ ...bot, wakeScope: "all" })).toMatchObject({ bot: { wakeScope: "all" } });
    const notification = {
      id: "schedule:s1:r1",
      workspaceId: "w",
      scope: null,
      kind: "schedule_failed",
      agentId: null,
      scheduleId: "s1",
      runId: "r1",
      terminalId: null,
      title: 'Schedule "nightly" run failed',
      summary: "REVOKED",
      at: "2026-11-12T00:00:00Z",
      conversationId: null,
    };
    const parsed = BotResponseSchema.parse({
      type: "bot.notifications.list.response",
      payload: {
        requestId: "r",
        headSequence: 3,
        seenSequence: null,
        wakeScope: "own",
        notifications: [
          {
            sequence: 3,
            admittedAt: "2026-11-12T00:00:01Z",
            notification,
            wake: { state: "skipped", reason: "over_budget" },
          },
          {
            sequence: 2,
            admittedAt: "2026-11-12T00:00:01Z",
            notification,
            wake: {
              state: "woke",
              conversationId: crypto.randomUUID(),
              turnId: crypto.randomUUID(),
            },
          },
          {
            sequence: 1,
            admittedAt: "2026-11-12T00:00:01Z",
            notification,
            wake: { state: "logged" },
          },
        ],
      },
    });
    expect(parsed.type).toBe("bot.notifications.list.response");
  });

  it("changes the wake scope alone and reads wake states from newer hosts", () => {
    expect(
      BotRequestSchema.parse({
        type: "bot.update.request",
        requestId: "r",
        workspaceId: "w",
        wakeScope: "all",
      }),
    ).not.toHaveProperty("execution");
    expect(BotSkipReasonSchema.parse("reported")).toBe("reported");
    for (const wake of [
      { state: "skipped", reason: "observed" },
      { state: "skipped", reason: "blocked" },
      { state: "skipped", reason: "reported" },
      { state: "skipped", reason: "reason_from_a_newer_host" },
      { state: "state_from_a_newer_host", detail: 1 },
    ]) {
      expect(BotWakeStatusSchema.parse(wake)).toEqual(wake);
    }
    expect(BotWakeStatusSchema.safeParse({ state: "woke" }).success).toBe(false);
    expect(BotWakeStatusSchema.safeParse({ state: "skipped" }).success).toBe(false);
    const entry = (sequence: number, wake: unknown) => ({
      sequence,
      admittedAt: "2026-11-12T00:00:01Z",
      notification: {
        id: `agent:a:${sequence}`,
        workspaceId: "w",
        kind: "agent_finished",
        title: "Agent finished",
        at: "2026-11-12T00:00:00Z",
      },
      wake,
    });
    const envelope = {
      type: "session",
      message: {
        type: "bot.notifications.list.response",
        payload: {
          requestId: "r",
          headSequence: 2,
          seenSequence: null,
          wakeScope: "own",
          notifications: [
            entry(2, { state: "state_from_a_newer_host" }),
            entry(1, { state: "skipped", reason: "reason_from_a_newer_host" }),
          ],
        },
      },
    };
    expect(validateWSOutboundMessage(envelope).success).toBe(true);
  });

  it("keeps tickets optional and reads notifications from hosts without them unchanged", () => {
    expect(BOT_TICKET_METHODS).toEqual([
      "bot.ticket.list.request",
      "bot.ticket.get.request",
      "bot.ticket.update.request",
    ]);
    for (const method of BOT_TICKET_METHODS) expect(BOT_METHODS).not.toContain(method);
    const legacy = {
      id: "agent:a:1",
      workspaceId: "w",
      scope: "w",
      kind: "agent_finished",
      agentId: "a",
      scheduleId: null,
      runId: null,
      terminalId: null,
      title: "Agent finished",
      summary: null,
      at: "2026-11-13T00:00:00Z",
      conversationId: null,
    };
    expect(BotNotificationSchema.parse(legacy)).toEqual(legacy);
    const ticketId = crypto.randomUUID();
    for (const kind of ["ticket_blocked", "ticket_done", "ticket_failed"]) {
      expect(
        BotNotificationSchema.parse({ ...legacy, kind, ticketId, ticketTitle: "Fix login" }),
      ).toMatchObject({ kind, ticketId, ticketTitle: "Fix login" });
    }
  });

  it("validates ticket requests and round-trips tickets, including values from newer hosts", () => {
    const ticketId = crypto.randomUUID();
    const base = { requestId: "r", workspaceId: "w" };
    expect(BotRequestSchema.parse({ type: "bot.ticket.list.request", ...base })).toEqual({
      type: "bot.ticket.list.request",
      ...base,
    });
    for (const patch of [{ limit: 0 }, { limit: 129 }, { includeClosed: "yes" }, { extra: 1 }]) {
      expect(
        BotRequestSchema.safeParse({ type: "bot.ticket.list.request", ...base, ...patch }).success,
      ).toBe(false);
    }
    expect(
      BotRequestSchema.safeParse({ type: "bot.ticket.get.request", ...base, ticketId: "t" })
        .success,
    ).toBe(false);
    const update = { type: "bot.ticket.update.request", ...base, ticketId };
    for (const status of ["open", "done", "failed", "cancelled"]) {
      expect(BotRequestSchema.safeParse({ ...update, status }).success).toBe(true);
    }
    // Only the host moves a ticket into waiting; renaming and notes belong to the Bot.
    expect(BotRequestSchema.safeParse({ ...update, status: "waiting" }).success).toBe(false);
    expect(BotRequestSchema.safeParse({ ...update, status: "done", title: "x" }).success).toBe(
      false,
    );
    const at = "2026-11-13T08:00:00Z";
    const report = {
      sequence: 2,
      at,
      source: "system",
      kind: "agent_permission",
      agentId: "agt_1",
      text: "",
      status: "waiting",
    };
    const summary = {
      id: ticketId,
      title: "Fix login",
      status: "waiting",
      waiting: { reason: "permission", agentId: "agt_1", since: at },
      conversationId: crypto.randomUUID(),
      createdAt: at,
      updatedAt: at,
      closedAt: null,
      outcome: null,
      assignments: [{ agentId: "agt_1", workspaceId: "w", assignedAt: at }],
      latestReport: report,
    };
    const { latestReport: _latest, ...fields } = summary;
    const full = { ...fields, request: "Fix the login form", reports: [report] };
    const newer = {
      ...summary,
      id: crypto.randomUUID(),
      status: "status_from_a_newer_host",
      waiting: { reason: "reason_from_a_newer_host", agentId: "agt_2", since: at, extra: 1 },
      latestReport: {
        ...report,
        source: "source_from_a_newer_host",
        kind: "kind_from_a_newer_host",
        status: "status_from_a_newer_host",
      },
    };
    const responses = [
      {
        type: "bot.ticket.list.response",
        payload: { requestId: "r", tickets: [summary, { ...summary, latestReport: null }, newer] },
      },
      { type: "bot.ticket.get.response", payload: { requestId: "r", ticket: full } },
      {
        type: "bot.ticket.update.response",
        payload: { requestId: "r", ticket: { ...full, status: "done" }, changed: true },
      },
    ];
    for (const response of responses) {
      expect(validateWSOutboundMessage({ type: "session", message: response }).success).toBe(true);
    }
    expect(BotResponseSchema.parse(responses[1])).toEqual(responses[1]);
    expect(BotResponseSchema.parse(responses[2])).toEqual(responses[2]);
    const parsed = BotResponseSchema.parse(responses[0]);
    expect(parsed.type === "bot.ticket.list.response" && parsed.payload.tickets[0]).toEqual(
      summary,
    );
    expect(parsed.type === "bot.ticket.list.response" && parsed.payload.tickets[2]).toMatchObject({
      status: "status_from_a_newer_host",
      waiting: { reason: "reason_from_a_newer_host" },
      latestReport: { kind: "kind_from_a_newer_host", source: "source_from_a_newer_host" },
    });
    // A summary from a host that omits optional fields still parses with an empty history.
    expect(
      BotTicketSummarySchema.parse({
        id: ticketId,
        title: "t",
        status: "open",
        createdAt: at,
        updatedAt: at,
      }),
    ).toMatchObject({ assignments: [] });
  });

  it("parses schedules that record the Bot conversation that created them", () => {
    const schedule = StoredScheduleSchema.parse({
      id: "s1",
      name: "nightly",
      prompt: "check",
      cadence: { type: "every", everyMs: 60_000 },
      target: { type: "agent", agentId: crypto.randomUUID() },
      status: "active",
      createdAt: "2026-11-12T00:00:00Z",
      updatedAt: "2026-11-12T00:00:00Z",
      nextRunAt: null,
      lastRunAt: null,
      pausedAt: null,
      expiresAt: null,
      maxRuns: null,
      runs: [],
      botScope: "w",
      botConversation: crypto.randomUUID(),
    });
    expect(schedule.id).toBe("s1");
  });
});

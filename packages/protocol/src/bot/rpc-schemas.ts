import { z } from "zod";
import {
  BotViewSchema,
  BotPermissionsSchema,
  BotExecutionSchema,
  BotChannelSchema,
  BotChannelConfigSchema,
  BotConversationSchema,
  BotTurnSchema,
  BotWakeScopeSchema,
  BotLoggedNotificationSchema,
} from "./types.js";

const WorkspaceIdSchema = z.string().trim().min(1);

const TelegramPairFailureSchema = z.enum([
  "invalidToken",
  "alreadyConnected",
  "webhookConflict",
  "pollingConflict",
  "expired",
  "unavailable",
]);

const ChannelScanFailureSchema = z.enum(["expired", "alreadyConnected", "unavailable"]);

const WebhookFailureSchema = z.enum([
  "centerOffline",
  "centerUnsupported",
  "automationUnavailable",
  "limitReached",
  "centerError",
]);

const ImessagePairFailureSchema = z.enum([
  "invalidKey",
  "invalidHandle",
  "dedicatedLine",
  "alreadyConnected",
  "expired",
  "unavailable",
]);

export const BOT_REQUEST_SCHEMAS = [
  z.strictObject({
    type: z.literal("bot.get.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
  }),
  z.strictObject({
    type: z.literal("bot.setup.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    execution: BotExecutionSchema,
    clearMemory: z.boolean().optional(),
    permissions: BotPermissionsSchema.optional(),
    // Hosts without Bot notifications reject the field; send it only when they report it.
    wakeScope: BotWakeScopeSchema.optional(),
  }),
  z.strictObject({
    type: z.literal("bot.update.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    // Unchanged when absent. Hosts without Bot notifications require it.
    execution: BotExecutionSchema.optional(),
    // Unchanged when absent.
    wakeScope: BotWakeScopeSchema.optional(),
  }),
  z.strictObject({
    type: z.literal("bot.permissions.update.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    permissions: BotPermissionsSchema,
  }),
  z.strictObject({
    type: z.literal("bot.pause.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
  }),
  z.strictObject({
    type: z.literal("bot.resume.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
  }),
  z.strictObject({
    type: z.literal("bot.remove.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
  }),
  z.strictObject({
    type: z.literal("bot.channel.list.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
  }),
  z.strictObject({ type: z.literal("bot.channel.types.request"), requestId: z.string() }),
  z.strictObject({
    type: z.literal("bot.channel.create.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    channelId: z.guid(),
    config: BotChannelConfigSchema,
    enabled: z.boolean().optional(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.update.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    channelId: z.guid(),
    config: BotChannelConfigSchema.optional(),
    enabled: z.boolean(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.delete.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    channelId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.status.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    channelId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.feishu.scan.start.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    allowGroups: z.boolean().optional(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.feishu.scan.status.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    scanId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.feishu.scan.cancel.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    scanId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.feishu.connect.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    channelId: z.guid(),
    appId: z.string(),
    appSecret: z.string(),
    allowedSenders: z.array(z.string()),
    allowGroups: z.boolean().optional(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.weixin.scan.start.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    allowGroups: z.boolean().optional(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.weixin.scan.status.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    scanId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.weixin.scan.cancel.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    scanId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.qq.scan.start.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    allowGroups: z.boolean().optional(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.qq.scan.status.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    scanId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.qq.scan.cancel.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    scanId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.qq.connect.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    channelId: z.guid(),
    appId: z.string(),
    appSecret: z.string(),
    allowGroups: z.boolean().optional(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.qq.pair.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    channelId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.webhook.create.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
  }),
  z.strictObject({
    type: z.literal("bot.channel.webhook.rotate.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    channelId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.telegram.pair.start.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    token: z.string(),
    allowGroups: z.boolean().optional(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.telegram.pair.status.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    pairingId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.telegram.pair.cancel.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    pairingId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.imessage.pair.start.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    projectId: z.string(),
    apiKey: z.string(),
    handle: z.string(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.imessage.pair.status.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    pairingId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.imessage.pair.cancel.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    pairingId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.conversation.list.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
  }),
  z.strictObject({
    type: z.literal("bot.conversation.open.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
  }),
  z.strictObject({
    type: z.literal("bot.conversation.reset.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    conversationId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.message.send.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    conversationId: z.guid(),
    messageId: z.string().min(1),
    text: z.string().min(1),
  }),
  z.strictObject({
    type: z.literal("bot.history.get.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    conversationId: z.guid(),
    afterSequence: z.number().int().nonnegative().optional(),
    limit: z.number().int().min(1).max(50).optional(),
  }),
  z.strictObject({
    type: z.literal("bot.turn.get.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    turnId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.turn.cancel.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    turnId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.notifications.list.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    // Only entries this conversation has not seen in a completed turn.
    unseenBy: z.guid().optional(),
    limit: z.number().int().min(1).max(128).optional(),
  }),
  z.strictObject({
    type: z.literal("bot.memory.get.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
  }),
  z.strictObject({
    type: z.literal("bot.memory.update.request"),
    requestId: z.string(),
    workspaceId: WorkspaceIdSchema,
    text: z.string(),
    expected: z.string(),
  }),
] as const;

export const BOT_RESPONSE_SCHEMAS = [
  z.object({
    type: z.literal("bot.get.response"),
    payload: z.object({ requestId: z.string(), bot: BotViewSchema.nullable() }),
  }),
  z.object({
    type: z.literal("bot.setup.response"),
    payload: z.object({ requestId: z.string(), bot: BotViewSchema, created: z.boolean() }),
  }),
  z.object({
    type: z.literal("bot.update.response"),
    payload: z.object({
      requestId: z.string(),
      bot: BotViewSchema,
      configurationAppliesToNewSessions: z.boolean(),
    }),
  }),
  z.object({
    type: z.literal("bot.permissions.update.response"),
    payload: z.object({
      requestId: z.string(),
      bot: BotViewSchema,
      appliesToNextTurn: z.boolean(),
    }),
  }),
  z.object({
    type: z.literal("bot.pause.response"),
    payload: z.object({ requestId: z.string(), bot: BotViewSchema }),
  }),
  z.object({
    type: z.literal("bot.resume.response"),
    payload: z.object({ requestId: z.string(), bot: BotViewSchema }),
  }),
  z.object({
    type: z.literal("bot.remove.response"),
    payload: z.object({ requestId: z.string(), bot: BotViewSchema }),
  }),
  z.object({
    type: z.literal("bot.channel.list.response"),
    payload: z.object({ requestId: z.string(), channels: z.array(BotChannelSchema) }),
  }),
  z.object({
    type: z.literal("bot.channel.types.response"),
    payload: z.object({
      requestId: z.string(),
      supported: z.array(z.enum(["builtin", "telegram", "feishu"])),
      email: z.object({ supported: z.boolean(), planned: z.boolean() }),
    }),
  }),
  z.object({
    type: z.literal("bot.channel.create.response"),
    payload: z.object({ requestId: z.string(), channel: BotChannelSchema, created: z.boolean() }),
  }),
  z.object({
    type: z.literal("bot.channel.update.response"),
    payload: z.object({ requestId: z.string(), channel: BotChannelSchema }),
  }),
  z.object({
    type: z.literal("bot.channel.delete.response"),
    payload: z.object({ requestId: z.string(), deleted: z.boolean(), channelId: z.guid() }),
  }),
  z.object({
    type: z.literal("bot.channel.status.response"),
    payload: z.object({
      requestId: z.string(),
      channelId: z.guid(),
      enabled: z.boolean(),
      connection: z.unknown(),
    }),
  }),
  z.object({
    type: z.literal("bot.channel.feishu.scan.start.response"),
    payload: z.object({
      requestId: z.string(),
      scanId: z.guid(),
      url: z.string(),
      expiresInSeconds: z.number(),
    }),
  }),
  z.object({
    type: z.literal("bot.channel.feishu.scan.status.response"),
    payload: z.object({
      requestId: z.string(),
      state: z.enum(["waiting", "complete", "failed"]),
      reason: z.enum(["denied", "expired", "larkUnsupported", "unavailable"]).optional(),
      channel: BotChannelSchema.optional(),
    }),
  }),
  z.object({
    type: z.literal("bot.channel.feishu.scan.cancel.response"),
    payload: z.object({ requestId: z.string(), cancelled: z.boolean() }),
  }),
  z.object({
    type: z.literal("bot.channel.feishu.connect.response"),
    payload: z.object({ requestId: z.string(), channel: BotChannelSchema, created: z.boolean() }),
  }),
  z.object({
    type: z.literal("bot.channel.webhook.create.response"),
    payload: z.object({
      requestId: z.string(),
      state: z.enum(["created", "failed"]),
      reason: WebhookFailureSchema.optional(),
      channel: BotChannelSchema.optional(),
      // Shown once: the center keeps it and the host never stores it.
      secret: z.string().optional(),
    }),
  }),
  z.object({
    type: z.literal("bot.channel.webhook.rotate.response"),
    payload: z.object({
      requestId: z.string(),
      state: z.enum(["rotated", "failed"]),
      reason: WebhookFailureSchema.optional(),
      secret: z.string().optional(),
      url: z.string().optional(),
    }),
  }),
  z.object({
    type: z.literal("bot.channel.telegram.pair.start.response"),
    payload: z.object({
      requestId: z.string(),
      state: z.enum(["waiting", "failed"]),
      reason: TelegramPairFailureSchema.optional(),
      pairingId: z.guid().optional(),
      botUsername: z.string().optional(),
      url: z.string().optional(),
      expiresInSeconds: z.number().optional(),
    }),
  }),
  z.object({
    type: z.literal("bot.channel.telegram.pair.status.response"),
    payload: z.object({
      requestId: z.string(),
      state: z.enum(["waiting", "complete", "failed"]),
      reason: TelegramPairFailureSchema.optional(),
      channel: BotChannelSchema.optional(),
    }),
  }),
  z.object({
    type: z.literal("bot.channel.telegram.pair.cancel.response"),
    payload: z.object({ requestId: z.string(), cancelled: z.boolean() }),
  }),
  z.object({
    type: z.literal("bot.channel.weixin.scan.start.response"),
    payload: z.object({
      requestId: z.string(),
      scanId: z.guid(),
      url: z.string(),
      expiresInSeconds: z.number(),
    }),
  }),
  z.object({
    type: z.literal("bot.channel.weixin.scan.status.response"),
    payload: z.object({
      requestId: z.string(),
      state: z.enum(["waiting", "complete", "failed"]),
      reason: ChannelScanFailureSchema.optional(),
      channel: BotChannelSchema.optional(),
    }),
  }),
  z.object({
    type: z.literal("bot.channel.weixin.scan.cancel.response"),
    payload: z.object({ requestId: z.string(), cancelled: z.boolean() }),
  }),
  z.object({
    type: z.literal("bot.channel.qq.scan.start.response"),
    payload: z.object({
      requestId: z.string(),
      scanId: z.guid(),
      url: z.string(),
      expiresInSeconds: z.number(),
    }),
  }),
  z.object({
    type: z.literal("bot.channel.qq.scan.status.response"),
    payload: z.object({
      requestId: z.string(),
      state: z.enum(["waiting", "complete", "failed"]),
      reason: ChannelScanFailureSchema.optional(),
      channel: BotChannelSchema.optional(),
    }),
  }),
  z.object({
    type: z.literal("bot.channel.qq.scan.cancel.response"),
    payload: z.object({ requestId: z.string(), cancelled: z.boolean() }),
  }),
  z.object({
    type: z.literal("bot.channel.qq.connect.response"),
    payload: z.object({ requestId: z.string(), channel: BotChannelSchema, created: z.boolean() }),
  }),
  z.object({
    type: z.literal("bot.channel.qq.pair.response"),
    payload: z.object({ requestId: z.string(), code: z.string(), expiresInSeconds: z.number() }),
  }),
  z.object({
    type: z.literal("bot.channel.imessage.pair.start.response"),
    payload: z.object({
      requestId: z.string(),
      state: z.enum(["waiting", "failed"]),
      reason: ImessagePairFailureSchema.optional(),
      pairingId: z.guid().optional(),
      phoneNumber: z.string().optional(),
      code: z.string().optional(),
      url: z.string().optional(),
      expiresInSeconds: z.number().optional(),
    }),
  }),
  z.object({
    type: z.literal("bot.channel.imessage.pair.status.response"),
    payload: z.object({
      requestId: z.string(),
      state: z.enum(["waiting", "complete", "failed"]),
      reason: ImessagePairFailureSchema.optional(),
      channel: BotChannelSchema.optional(),
    }),
  }),
  z.object({
    type: z.literal("bot.channel.imessage.pair.cancel.response"),
    payload: z.object({ requestId: z.string(), cancelled: z.boolean() }),
  }),
  z.object({
    type: z.literal("bot.conversation.list.response"),
    payload: z.object({ requestId: z.string(), conversations: z.array(BotConversationSchema) }),
  }),
  z.object({
    type: z.literal("bot.conversation.open.response"),
    payload: z.object({ requestId: z.string(), conversation: BotConversationSchema }),
  }),
  z.object({
    type: z.literal("bot.conversation.reset.response"),
    payload: z.object({ requestId: z.string(), conversation: BotConversationSchema }),
  }),
  z.object({
    type: z.literal("bot.message.send.response"),
    payload: z.object({
      requestId: z.string(),
      turn: BotTurnSchema,
      accepted: z.boolean(),
      duplicate: z.boolean(),
    }),
  }),
  z.object({
    type: z.literal("bot.history.get.response"),
    payload: z.object({
      requestId: z.string(),
      turns: z.array(BotTurnSchema),
      nextSequence: z.number().int().nonnegative().nullable(),
    }),
  }),
  z.object({
    type: z.literal("bot.turn.get.response"),
    payload: z.object({ requestId: z.string(), turn: BotTurnSchema }),
  }),
  z.object({
    type: z.literal("bot.turn.cancel.response"),
    payload: z.union([
      z.object({ requestId: z.string(), turn: BotTurnSchema }),
      z.object({ requestId: z.string(), cancellationRequested: z.boolean() }),
    ]),
  }),
  z.object({
    type: z.literal("bot.notifications.list.response"),
    payload: z.object({
      requestId: z.string(),
      headSequence: z.number().int().nonnegative(),
      seenSequence: z.number().int().nonnegative().nullable(),
      wakeScope: BotWakeScopeSchema,
      // Newest first.
      notifications: z.array(BotLoggedNotificationSchema),
    }),
  }),
  z.object({
    type: z.literal("bot.memory.get.response"),
    payload: z.object({ requestId: z.string(), text: z.string(), revision: z.string() }),
  }),
  z.object({
    type: z.literal("bot.memory.update.response"),
    payload: z.object({ requestId: z.string(), revision: z.string() }),
  }),
] as const;

export const BotRequestSchema = z.discriminatedUnion("type", BOT_REQUEST_SCHEMAS);
export const BotResponseSchema = z.discriminatedUnion("type", BOT_RESPONSE_SCHEMAS);
export type BotRequest = z.infer<typeof BotRequestSchema>;
export type BotResponse = z.infer<typeof BotResponseSchema>;
export type BotMethod = BotRequest["type"];
export type BotParams<M extends BotMethod> = Omit<
  Extract<BotRequest, { type: M }>,
  "type" | "requestId"
>;
export type BotResponseType<M extends BotMethod> = M extends `${infer Prefix}.request`
  ? `${Prefix}.response`
  : never;
export type BotResult<M extends BotMethod> = Extract<
  BotResponse,
  { type: BotResponseType<M> }
>["payload"];
/** Methods added after the core v2 Bot set; hosts without them keep the Bot tab. */
export const BOT_PERMISSION_METHODS = ["bot.permissions.update.request"] as const;
/** Notification log reads and the wake-scope setting; older hosts keep the Bot without them. */
export const BOT_NOTIFICATION_METHODS = ["bot.notifications.list.request"] as const;
export const BOT_FEISHU_SETUP_METHODS = [
  "bot.channel.feishu.scan.start.request",
  "bot.channel.feishu.scan.status.request",
  "bot.channel.feishu.scan.cancel.request",
  "bot.channel.feishu.connect.request",
] as const;
export const BOT_TELEGRAM_SETUP_METHODS = [
  "bot.channel.telegram.pair.start.request",
  "bot.channel.telegram.pair.status.request",
  "bot.channel.telegram.pair.cancel.request",
] as const;
export const BOT_WEIXIN_SETUP_METHODS = [
  "bot.channel.weixin.scan.start.request",
  "bot.channel.weixin.scan.status.request",
  "bot.channel.weixin.scan.cancel.request",
] as const;
export const BOT_QQ_SETUP_METHODS = [
  "bot.channel.qq.scan.start.request",
  "bot.channel.qq.scan.status.request",
  "bot.channel.qq.scan.cancel.request",
  "bot.channel.qq.connect.request",
  "bot.channel.qq.pair.request",
] as const;
export const BOT_IMESSAGE_SETUP_METHODS = [
  "bot.channel.imessage.pair.start.request",
  "bot.channel.imessage.pair.status.request",
  "bot.channel.imessage.pair.cancel.request",
] as const;
export const BOT_WEBHOOK_SETUP_METHODS = [
  "bot.channel.webhook.create.request",
  "bot.channel.webhook.rotate.request",
] as const;
const OPTIONAL_BOT_METHODS: readonly string[] = [
  ...BOT_PERMISSION_METHODS,
  ...BOT_NOTIFICATION_METHODS,
  ...BOT_FEISHU_SETUP_METHODS,
  ...BOT_TELEGRAM_SETUP_METHODS,
  ...BOT_WEIXIN_SETUP_METHODS,
  ...BOT_QQ_SETUP_METHODS,
  ...BOT_IMESSAGE_SETUP_METHODS,
  ...BOT_WEBHOOK_SETUP_METHODS,
];
export const BOT_METHODS = BOT_REQUEST_SCHEMAS.map((schema) => schema.shape.type.value).filter(
  (method) => !OPTIONAL_BOT_METHODS.includes(method),
);
export const ALL_BOT_METHODS = BOT_REQUEST_SCHEMAS.map((schema) => schema.shape.type.value);

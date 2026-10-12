import { z } from "zod";
import {
  BotSchema,
  BotSummarySchema,
  BotExecutionSchema,
  BotChannelSchema,
  BotChannelConfigSchema,
  BotConversationSchema,
  BotTurnSchema,
} from "./types.js";

export const BOT_REQUEST_SCHEMAS = [
  z.strictObject({
    type: z.literal("bot.create.request"),
    requestId: z.string(),
    botId: z.guid(),
    workspaceId: z.string().trim().min(1),
    name: z.string().trim().min(1),
    instructions: z.string().optional(),
    execution: BotExecutionSchema,
  }),
  z.strictObject({
    type: z.literal("bot.list.request"),
    requestId: z.string(),
    workspaceId: z.string().trim().min(1),
  }),
  z.strictObject({ type: z.literal("bot.get.request"), requestId: z.string(), botId: z.guid() }),
  z.strictObject({
    type: z.literal("bot.update.request"),
    requestId: z.string(),
    botId: z.guid(),
    name: z.string().trim().min(1).optional(),
    instructions: z.string().optional(),
    execution: BotExecutionSchema.optional(),
    enabled: z.boolean().optional(),
  }),
  z.strictObject({
    type: z.literal("bot.archive.request"),
    requestId: z.string(),
    botId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.list.request"),
    requestId: z.string(),
    botId: z.guid(),
  }),
  z.strictObject({ type: z.literal("bot.channel.types.request"), requestId: z.string() }),
  z.strictObject({
    type: z.literal("bot.channel.create.request"),
    requestId: z.string(),
    botId: z.guid(),
    channelId: z.guid(),
    config: BotChannelConfigSchema,
    enabled: z.boolean().optional(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.update.request"),
    requestId: z.string(),
    botId: z.guid(),
    channelId: z.guid(),
    config: BotChannelConfigSchema.optional(),
    enabled: z.boolean(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.delete.request"),
    requestId: z.string(),
    botId: z.guid(),
    channelId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.channel.status.request"),
    requestId: z.string(),
    botId: z.guid(),
    channelId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.conversation.list.request"),
    requestId: z.string(),
    botId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.conversation.open.request"),
    requestId: z.string(),
    botId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.conversation.reset.request"),
    requestId: z.string(),
    botId: z.guid(),
    conversationId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.message.send.request"),
    requestId: z.string(),
    botId: z.guid(),
    conversationId: z.guid(),
    messageId: z.string().min(1),
    text: z.string().min(1),
  }),
  z.strictObject({
    type: z.literal("bot.history.get.request"),
    requestId: z.string(),
    botId: z.guid(),
    conversationId: z.guid(),
    afterSequence: z.number().int().nonnegative().optional(),
    limit: z.number().int().min(1).max(50).optional(),
  }),
  z.strictObject({
    type: z.literal("bot.turn.get.request"),
    requestId: z.string(),
    botId: z.guid(),
    turnId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.turn.cancel.request"),
    requestId: z.string(),
    botId: z.guid(),
    turnId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.permission.resolve.request"),
    requestId: z.string(),
    botId: z.guid(),
    turnId: z.guid(),
    permissionId: z.string().min(1),
    response: z.unknown(),
  }),
  z.strictObject({
    type: z.literal("bot.memory.get.request"),
    requestId: z.string(),
    botId: z.guid(),
  }),
  z.strictObject({
    type: z.literal("bot.memory.update.request"),
    requestId: z.string(),
    botId: z.guid(),
    text: z.string(),
    expected: z.string(),
  }),
] as const;

export const BOT_RESPONSE_SCHEMAS = [
  z.object({
    type: z.literal("bot.create.response"),
    payload: z.object({ requestId: z.string(), bot: BotSchema, created: z.boolean() }),
  }),
  z.object({
    type: z.literal("bot.list.response"),
    payload: z.object({ requestId: z.string(), bots: z.array(BotSummarySchema) }),
  }),
  z.object({
    type: z.literal("bot.get.response"),
    payload: z.object({ requestId: z.string(), bot: BotSchema }),
  }),
  z.object({
    type: z.literal("bot.update.response"),
    payload: z.object({ requestId: z.string(), bot: BotSchema }),
  }),
  z.object({
    type: z.literal("bot.archive.response"),
    payload: z.object({ requestId: z.string(), bot: BotSchema }),
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
    type: z.literal("bot.permission.resolve.response"),
    payload: z.object({ requestId: z.string(), resolved: z.boolean() }),
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
export const BOT_METHODS = BOT_REQUEST_SCHEMAS.map((schema) => schema.shape.type.value);

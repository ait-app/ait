import { z } from "zod";

export const BotExecutionSchema = z.strictObject({
  provider: z.string().trim().min(1),
  model: z.string().nullable(),
  modeId: z.string().nullable(),
  thinking: z.string().nullable(),
});
export const BotSummarySchema = z.object({
  id: z.guid(),
  workspaceId: z.string(),
  name: z.string(),
  execution: BotExecutionSchema,
  builtinChannelId: z.guid(),
  enabled: z.boolean(),
  archivedAt: z.string().nullable(),
  createdAt: z.string(),
});
export const BotSchema = BotSummarySchema.extend({ instructions: z.string() });
export const BotChannelConfigSchema = z.discriminatedUnion("kind", [
  z.strictObject({ kind: z.literal("builtin") }),
  z.strictObject({
    kind: z.literal("telegram"),
    tokenRef: z.string(),
    allowedSenders: z.array(z.string()),
    allowGroups: z.boolean().default(false),
  }),
  z.strictObject({
    kind: z.literal("feishu"),
    appId: z.string(),
    secretRef: z.string(),
    allowedSenders: z.array(z.string()),
    allowGroups: z.boolean().default(false),
  }),
]);
export const BotChannelSchema = z.object({
  id: z.guid(),
  botId: z.guid(),
  config: BotChannelConfigSchema,
  enabled: z.boolean(),
  cursor: z.number().nullable(),
  deleted: z.boolean().default(false),
});
export const BotConversationSchema = z.object({
  id: z.guid(),
  botId: z.guid(),
  channelId: z.guid(),
  peer: z.string(),
  sender: z.string(),
  thread: z.string().nullable(),
  generation: z.number().int().nonnegative(),
  nextSequence: z.number().int().nonnegative(),
});
export const BotTurnSchema = z.object({
  id: z.guid(),
  conversationId: z.guid(),
  messageId: z.string(),
  sequence: z.number().int().nonnegative(),
  generation: z.number().int().nonnegative(),
  text: z.string(),
  replyTo: z.string().nullable(),
  reply: z.string().nullable(),
  pendingPermissions: z.array(z.unknown()).default([]),
  status: z.enum([
    "queued",
    "sending",
    "running",
    "needs_approval",
    "completed",
    "failed",
    "uncertain",
    "cancelled",
  ]),
  delivery: z.enum(["pending", "sending", "delivered", "uncertain"]),
  error: z.string().nullable(),
  createdAt: z.string(),
});

export type Bot = z.infer<typeof BotSchema>;
export type BotSummary = z.infer<typeof BotSummarySchema>;
export type BotExecution = z.infer<typeof BotExecutionSchema>;
export type BotConversation = z.infer<typeof BotConversationSchema>;
export type BotTurn = z.infer<typeof BotTurnSchema>;

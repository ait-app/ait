import { z } from "zod";

export const BotExecutionSchema = z.strictObject({
  provider: z.string().trim().min(1),
  model: z.string().nullable(),
  modeId: z.string().nullable(),
  thinking: z.string().nullable(),
});
export const BotStateSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("active") }),
  z.object({ kind: z.literal("paused"), reason: z.enum(["user", "directory_missing"]) }),
  z.object({ kind: z.literal("retired"), at: z.string() }),
]);
export const BOT_TOOL_GROUPS = [
  "observe",
  "agents",
  "stop",
  "schedules",
  "scripts",
  "terminals",
] as const;
// `workspaces` is retired but may still appear in profiles saved earlier or sent by older hosts.
export const BotToolGroupSchema = z.enum([...BOT_TOOL_GROUPS, "workspaces"]);
export const BotPermissionsSchema = z.strictObject({
  groups: z.array(BotToolGroupSchema).max(7),
  modes: z.record(z.string().min(1), z.array(z.string().min(1)).min(1).max(16)),
});
export const BotViewSchema = z.object({
  workspaceId: z.string(),
  execution: BotExecutionSchema,
  builtinChannelId: z.guid(),
  state: BotStateSchema,
  // Absent on hosts that predate Bot permissions.
  permissions: BotPermissionsSchema.optional(),
  createdAt: z.string(),
});
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
  z.strictObject({
    kind: z.literal("weixin"),
    accountId: z.string(),
    baseUrl: z.string(),
    tokenRef: z.string(),
    allowedSenders: z.array(z.string()),
  }),
  z.strictObject({
    kind: z.literal("qq"),
    appId: z.string(),
    secretRef: z.string(),
    allowedSenders: z.array(z.string()),
    allowGroups: z.boolean().default(false),
  }),
  z.strictObject({
    kind: z.literal("photon_imessage"),
    projectId: z.string(),
    keyRef: z.string(),
    allowedSenders: z.array(z.string()),
  }),
  // The Ait center relays chat messages through its own Hub session.
  z.strictObject({ kind: z.literal("hub") }),
]);
export const BotChannelSchema = z.object({
  id: z.guid(),
  config: BotChannelConfigSchema,
  enabled: z.boolean(),
  cursor: z.number().nullable(),
  deleted: z.boolean().default(false),
});
export const BotConversationSchema = z.object({
  id: z.guid(),
  channelId: z.guid(),
  peer: z.string(),
  sender: z.string(),
  thread: z.string().nullable(),
  generation: z.number().int().nonnegative(),
  nextSequence: z.number().int().nonnegative(),
  // Absent on hosts that predate read-only inspection of the Bot's session.
  sessionAgentId: z.string().optional(),
});
export const BotTurnMessageSchema = z.object({
  id: z.string(),
  text: z.string(),
  status: z.enum(["streaming", "completed"]),
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
  messages: z.array(BotTurnMessageSchema).default([]),
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

export type BotToolGroup = z.infer<typeof BotToolGroupSchema>;
export type BotPermissions = z.infer<typeof BotPermissionsSchema>;
export type BotView = z.infer<typeof BotViewSchema>;
export type BotState = z.infer<typeof BotStateSchema>;
export type BotExecution = z.infer<typeof BotExecutionSchema>;
export type BotConversation = z.infer<typeof BotConversationSchema>;
export type BotTurnMessage = z.infer<typeof BotTurnMessageSchema>;
export type BotTurn = z.infer<typeof BotTurnSchema>;

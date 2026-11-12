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
/** Which Workspace notifications wake the Bot: its own work only, or the whole Workspace. */
export const BotWakeScopeSchema = z.enum(["own", "all"]);
export const BOT_NOTIFICATION_KINDS = [
  "agent_finished",
  "agent_failed",
  "agent_permission",
  "schedule_succeeded",
  "schedule_failed",
  "terminal_attention",
] as const;
export const BotNotificationKindSchema = z.enum(BOT_NOTIFICATION_KINDS);
/** A Workspace event offered to the Bot; optional subject fields are `null` when absent. */
export const BotNotificationSchema = z.object({
  id: z.string(),
  workspaceId: z.string(),
  scope: z.string().nullish(),
  // A kind added by a later host must not make the whole history unreadable.
  kind: z.union([BotNotificationKindSchema, z.string()]),
  agentId: z.string().nullish(),
  scheduleId: z.string().nullish(),
  runId: z.string().nullish(),
  terminalId: z.string().nullish(),
  title: z.string(),
  summary: z.string().nullish(),
  at: z.string(),
  conversationId: z.string().nullish(),
});
const BOT_KNOWN_WAKE_STATES: ReadonlySet<string> = new Set([
  "logged",
  "pending",
  "woke",
  "skipped",
]);
export const BOT_SKIP_REASONS = [
  "over_budget",
  "inbox_full",
  "paused",
  "observed",
  "blocked",
] as const;
export const BotSkipReasonSchema = z.enum(BOT_SKIP_REASONS);
/** A wake state or skip reason added by a later host is kept as text instead of failing. */
export const BotWakeStatusSchema = z.union([
  z.discriminatedUnion("state", [
    z.object({ state: z.literal("logged") }),
    z.object({ state: z.literal("pending") }),
    z.object({ state: z.literal("woke"), conversationId: z.string(), turnId: z.string() }),
    z.object({
      state: z.literal("skipped"),
      reason: z.union([BotSkipReasonSchema, z.string()]),
    }),
  ]),
  z.looseObject({
    state: z.string().refine((state) => !BOT_KNOWN_WAKE_STATES.has(state)),
  }),
]);
export const BotLoggedNotificationSchema = z.object({
  sequence: z.number().int().nonnegative(),
  admittedAt: z.string(),
  notification: BotNotificationSchema,
  wake: BotWakeStatusSchema,
});
export const BotViewSchema = z.object({
  workspaceId: z.string(),
  execution: BotExecutionSchema,
  builtinChannelId: z.guid(),
  state: BotStateSchema,
  // Absent on hosts that predate Bot permissions.
  permissions: BotPermissionsSchema.optional(),
  // Absent on hosts that predate Bot notifications.
  wakeScope: BotWakeScopeSchema.optional(),
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
  // An HTTP endpoint the Ait center hosts; the center keeps its secret.
  z.strictObject({ kind: z.literal("webhook"), endpointId: z.string(), url: z.string() }),
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
  // Present only on turns started by Workspace notifications rather than user input.
  wake: z.object({ notifications: z.array(BotNotificationSchema) }).optional(),
  // The Bot answered a wake turn with its silent reply, so nothing was delivered.
  silent: z.boolean().optional(),
  notificationCursor: z.number().int().nonnegative().optional(),
});

export type BotToolGroup = z.infer<typeof BotToolGroupSchema>;
export type BotPermissions = z.infer<typeof BotPermissionsSchema>;
export type BotView = z.infer<typeof BotViewSchema>;
export type BotState = z.infer<typeof BotStateSchema>;
export type BotExecution = z.infer<typeof BotExecutionSchema>;
export type BotConversation = z.infer<typeof BotConversationSchema>;
export type BotTurnMessage = z.infer<typeof BotTurnMessageSchema>;
export type BotTurn = z.infer<typeof BotTurnSchema>;
export type BotWakeScope = z.infer<typeof BotWakeScopeSchema>;
export type BotNotificationKind = z.infer<typeof BotNotificationKindSchema>;
export type BotNotification = z.infer<typeof BotNotificationSchema>;
export type BotWakeStatus = z.infer<typeof BotWakeStatusSchema>;
export type BotLoggedNotification = z.infer<typeof BotLoggedNotificationSchema>;

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
  "ticket_blocked",
  "ticket_done",
  "ticket_failed",
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
  // Present when the subject works on one of this Bot's tickets; absent on hosts without tickets.
  ticketId: z.string().nullish(),
  ticketTitle: z.string().nullish(),
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
  "reported",
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
export const BOT_TICKET_STATUSES = ["open", "waiting", "done", "failed", "cancelled"] as const;
export const BotTicketStatusSchema = z.enum(BOT_TICKET_STATUSES);
/** Statuses a user may set: reopen, or close as done, failed or cancelled. */
export const BotTicketUserStatusSchema = z.enum(["open", "done", "failed", "cancelled"]);
export const BOT_TICKET_WAIT_REASONS = ["permission", "blocked"] as const;
export const BOT_TICKET_REPORT_SOURCES = ["agent", "bot", "user", "system"] as const;
export const BOT_TICKET_REPORT_KINDS = [
  "progress",
  "blocked",
  "done",
  "failed",
  "agent_finished",
  "agent_failed",
  "agent_permission",
  "note",
  "status",
] as const;
// Statuses, wait reasons, report sources and kinds a later host adds are kept as text, so one
// new value never makes the whole ticket list unreadable.
const TicketStatusValueSchema = z.union([BotTicketStatusSchema, z.string()]);
/** What a waiting ticket waits for: an Agent's permission request or a worker reporting blocked. */
export const BotTicketWaitingSchema = z.object({
  reason: z.union([z.enum(BOT_TICKET_WAIT_REASONS), z.string()]),
  agentId: z.string(),
  since: z.string(),
});
/** An Agent working on a ticket; its live state comes from the Agent directory. */
export const BotTicketAssignmentSchema = z.object({
  agentId: z.string(),
  workspaceId: z.string(),
  assignedAt: z.string(),
});
/** One dated entry in a ticket's history; `status` is set when the entry changed the status. */
export const BotTicketReportSchema = z.object({
  sequence: z.number().int().nonnegative(),
  at: z.string(),
  source: z.union([z.enum(BOT_TICKET_REPORT_SOURCES), z.string()]),
  kind: z.union([z.enum(BOT_TICKET_REPORT_KINDS), z.string()]),
  agentId: z.string().nullish(),
  text: z.string().default(""),
  status: TicketStatusValueSchema.nullish(),
});
const BotTicketFieldsSchema = z.object({
  id: z.string(),
  title: z.string(),
  status: TicketStatusValueSchema,
  waiting: BotTicketWaitingSchema.nullish(),
  conversationId: z.string().nullish(),
  createdAt: z.string(),
  updatedAt: z.string(),
  closedAt: z.string().nullish(),
  outcome: z.string().nullish(),
  assignments: z.array(BotTicketAssignmentSchema).default([]),
});
/** A ticket as `bot.ticket.list` returns it: no request or history, only the newest report. */
export const BotTicketSummarySchema = BotTicketFieldsSchema.extend({
  latestReport: BotTicketReportSchema.nullish(),
});
/** A ticket with what was asked and the kept history, oldest report first. */
export const BotTicketSchema = BotTicketFieldsSchema.extend({
  request: z.string(),
  reports: z.array(BotTicketReportSchema).default([]),
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
    kind: z.enum(["feishu", "lark"]),
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
  // The `kind` of the conversation's channel configuration, also once the channel is deleted;
  // absent on hosts that predate reading other conversations (ADR-280).
  channelKind: z.string().nullish(),
  // When the conversation's newest turn was admitted; null before its first turn.
  lastActivityAt: z.string().nullish(),
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
export type BotExecution = z.infer<typeof BotExecutionSchema>;
export type BotConversation = z.infer<typeof BotConversationSchema>;
export type BotTurnMessage = z.infer<typeof BotTurnMessageSchema>;
export type BotTurn = z.infer<typeof BotTurnSchema>;
export type BotWakeScope = z.infer<typeof BotWakeScopeSchema>;
export type BotNotification = z.infer<typeof BotNotificationSchema>;
export type BotWakeStatus = z.infer<typeof BotWakeStatusSchema>;
export type BotLoggedNotification = z.infer<typeof BotLoggedNotificationSchema>;
export type BotTicketUserStatus = z.infer<typeof BotTicketUserStatusSchema>;
export type BotTicketWaiting = z.infer<typeof BotTicketWaitingSchema>;
export type BotTicketAssignment = z.infer<typeof BotTicketAssignmentSchema>;
export type BotTicketReport = z.infer<typeof BotTicketReportSchema>;
export type BotTicketSummary = z.infer<typeof BotTicketSummarySchema>;
export type BotTicket = z.infer<typeof BotTicketSchema>;

import { z } from "zod";

/**
 * Bounded BTW side-question wire contract.
 *
 * Mirrors `crates/provider/core/src/protocol/side_questions.rs`: a topic freezes the
 * display context already persisted at submit, then accumulates question/answer
 * turns. Follow-ups reuse the frozen snapshot plus prior turns; they never
 * recapture main-conversation context. The daemon owns topic identity, receipts
 * and the bounded projection; clients render it and never re-derive it.
 */

/** Projection bounds mirrored from the daemon contract. */
export const SIDE_QUESTION_LIMITS = {
  /** Maximum retained topics in one projection. */
  topics: 8,
  /** Maximum turns per topic in the live projection, including the root turn. */
  turns: 8,
  /** Hard byte budget for one projected topic record. */
  recordBytes: 64 * 1024,
  /** Hard byte budget for one question. */
  questionBytes: 4 * 1024,
  /** Hard byte budget for one answer. */
  answerBytes: 48 * 1024,
  /** Hard byte budget for one redacted error. */
  errorBytes: 4 * 1024,
  /** Hard byte budget for the frozen display-context snapshot. */
  snapshotBytes: 64 * 1024,
} as const;

/** Stable rejection reasons returned by the ask RPC. */
export const SIDE_QUESTION_REASONS = {
  /** Another side question is still running for this Agent. */
  busy: "busy",
  /** The topic's latest turn is not complete; follow-up is not allowed yet. */
  topicNotSettled: "topic_not_settled",
  /** The follow-up references an unknown topic. */
  unknownTopic: "unknown_topic",
  /** The provider does not support side questions on this host. */
  unsupportedProvider: "unsupported_provider",
  /** The question is empty or exceeds the byte bound. */
  invalidQuestion: "invalid_question",
  /** Full side history reached the provider context budget; promotion remains available. */
  contextLimit: "context_limit",
} as const;

/** Byte length of a string's UTF-8 encoding, matching the Rust bound. */
export function utf8ByteLength(value: string): number {
  return new TextEncoder().encode(value).length;
}

const boundedText = (bytes: number) =>
  z.string().refine((value) => utf8ByteLength(value) <= bytes, {
    message: `exceeds ${bytes} UTF-8 bytes`,
  });

/** Turn lifecycle; `interrupted` means the auxiliary process died mid-turn. */
export const SideTurnStatusSchema = z.enum([
  "running",
  "complete",
  "cancelled",
  "error",
  "interrupted",
]);
export type SideTurnStatus = z.infer<typeof SideTurnStatusSchema>;

/** Execution strategy frozen once per topic at submit. */
export const SideStrategySchema = z.enum(["native", "isolated"]);
export type SideStrategy = z.infer<typeof SideStrategySchema>;

/** Durable acceptance receipt; `uncertain` never auto-retries. */
export const SideReceiptStateSchema = z.enum(["accepted", "rejected", "uncertain"]);
export type SideReceiptState = z.infer<typeof SideReceiptStateSchema>;

/** Native image content retained in BTW history. */
export const BtwImageSchema = z.object({
  type: z.string(),
  data: z.string(),
  mimeType: z.string(),
});
export type BtwImage = z.infer<typeof BtwImageSchema>;

/** One native question/answer turn inside a topic. */
export const BtwHistoryTurnSchema = z.object({
  question: boundedText(SIDE_QUESTION_LIMITS.questionBytes),
  images: z.array(BtwImageSchema).optional(),
  answer: boundedText(SIDE_QUESTION_LIMITS.answerBytes),
  status: SideTurnStatusSchema,
  createdAt: z.number().int().nonnegative(),
  updatedAt: z.number().int().nonnegative(),
  error: boundedText(SIDE_QUESTION_LIMITS.errorBytes).optional(),
});
export type BtwHistoryTurn = z.infer<typeof BtwHistoryTurnSchema>;

/**
 * Native-compatible topic record. The root status keeps the previous turn's
 * value while a follow-up runs, so consumers must use the latest turn.
 */
export const BtwHistoryRecordSchema = z.object({
  id: z.string().min(1),
  question: boundedText(SIDE_QUESTION_LIMITS.questionBytes),
  images: z.array(BtwImageSchema).optional(),
  answer: boundedText(SIDE_QUESTION_LIMITS.answerBytes),
  status: SideTurnStatusSchema,
  createdAt: z.number().int().nonnegative(),
  updatedAt: z.number().int().nonnegative(),
  error: boundedText(SIDE_QUESTION_LIMITS.errorBytes).optional(),
  leafId: z.string().nullable(),
  title: z.string().optional(),
  // The bound counts the root turn too, so follow-ups can never reach MAX_TURNS.
  followUps: z
    .array(BtwHistoryTurnSchema)
    .max(SIDE_QUESTION_LIMITS.turns - 1)
    .optional(),
});
export type BtwHistoryRecord = z.infer<typeof BtwHistoryRecordSchema>;

/** Topic projection: native record fields stay at the wire root. */
export const SideTopicSchema = BtwHistoryRecordSchema.extend({
  strategy: SideStrategySchema,
  truncated: z.boolean(),
});
export type SideTopic = z.infer<typeof SideTopicSchema>;

/** One readable display-context entry frozen into a topic snapshot. */
export const SideContextEntrySchema = z.object({
  role: z.string(),
  text: z.string(),
});
export type SideContextEntry = z.infer<typeof SideContextEntrySchema>;

/**
 * Frozen display context written once per topic at the first ask. Session
 * transfer reuses it verbatim; the version guards future store changes.
 */
export const SideContextSnapshotSchema = z.object({
  v: z.number().int().nonnegative(),
  agentId: z.string(),
  provider: z.string(),
  model: z.string().nullable(),
  nativeSessionId: z.string().optional(),
  nativeLeafId: z.string().optional(),
  capturedAt: z.number().int().nonnegative(),
  entries: z.array(SideContextEntrySchema),
  truncated: z.boolean(),
});
export type SideContextSnapshot = z.infer<typeof SideContextSnapshotSchema>;

/** Bounded projection pushed inside `providerUi.sideQuestions`. */
export const SideQuestionsStateSchema = z
  .object({
    records: z.array(SideTopicSchema).max(SIDE_QUESTION_LIMITS.topics),
    available: z.boolean().optional(),
    pendingAcceptance: z.boolean().optional(),
    unavailableReason: boundedText(SIDE_QUESTION_LIMITS.errorBytes).optional(),
    error: boundedText(SIDE_QUESTION_LIMITS.errorBytes).optional(),
    omitted: z.number().int().nonnegative(),
    reveal: z.string().optional(),
    runningId: z.string().optional(),
  })
  // Future provider features may extend this surface; an older client must not drop the
  // whole snapshot because a newer host added a member here.
  .passthrough();
export type SideQuestionsState = z.infer<typeof SideQuestionsStateSchema>;

/**
 * Minimal provider-UI state surface. Additive and optional: old clients ignore
 * it, and future provider features extend this same object.
 */
export const ProviderUiStateSchema = z
  .object({
    sideQuestions: SideQuestionsStateSchema.optional(),
  })
  .passthrough();
export type ProviderUiState = z.infer<typeof ProviderUiStateSchema>;

/** `agent.side_question.ask.request` payload. */
export const SideAskRequestSchema = z.strictObject({
  type: z.literal("agent.side_question.ask.request"),
  /** Idempotency key; a replay returns the stored receipt and never re-executes. */
  requestId: z.string().min(1),
  agentId: z.string(),
  question: z
    .string()
    .min(1)
    .refine((value) => utf8ByteLength(value) <= SIDE_QUESTION_LIMITS.questionBytes, {
      message: `exceeds ${SIDE_QUESTION_LIMITS.questionBytes} UTF-8 bytes`,
    }),
  /** Existing topic id for a follow-up; absent starts a new topic. */
  recordId: z.string().optional(),
});
export type SideAskRequest = z.infer<typeof SideAskRequestSchema>;

/** `agent.side_question.ask.response` payload. */
export const SideAskResponseSchema = z.object({
  receipt: SideReceiptStateSchema,
  recordId: z.string().optional(),
  strategy: SideStrategySchema.optional(),
  reason: z.string().optional(),
});
export type SideAskResponse = z.infer<typeof SideAskResponseSchema>;

/** `agent.side_question.cancel.request` payload. */
export const SideCancelRequestSchema = z.strictObject({
  type: z.literal("agent.side_question.cancel.request"),
  requestId: z.string().min(1),
  agentId: z.string(),
  /** Topic to cancel; absent cancels the running topic. */
  recordId: z.string().optional(),
});
export type SideCancelRequest = z.infer<typeof SideCancelRequestSchema>;

/** `agent.side_question.cancel.response` payload. */
export const SideCancelResponseSchema = z.object({
  cancelled: z.boolean(),
});
export type SideCancelResponse = z.infer<typeof SideCancelResponseSchema>;

/** `agent.side_question.history.request` payload. */
export const SideHistoryRequestSchema = z.strictObject({
  type: z.literal("agent.side_question.history.request"),
  requestId: z.string().min(1),
  agentId: z.string(),
});
export type SideHistoryRequest = z.infer<typeof SideHistoryRequestSchema>;

/** Outbound envelopes carry the correlated request id the SDK injects. */
export const SideAskResponseMessageSchema = z.object({
  type: z.literal("agent.side_question.ask.response"),
  payload: SideAskResponseSchema.extend({ requestId: z.string() }),
});
export type SideAskResponseMessage = z.infer<typeof SideAskResponseMessageSchema>;

export const SideCancelResponseMessageSchema = z.object({
  type: z.literal("agent.side_question.cancel.response"),
  payload: SideCancelResponseSchema.extend({ requestId: z.string() }),
});
export type SideCancelResponseMessage = z.infer<typeof SideCancelResponseMessageSchema>;

export const SideHistoryResponseMessageSchema = z.object({
  type: z.literal("agent.side_question.history.response"),
  payload: SideQuestionsStateSchema.extend({ requestId: z.string() }),
});
export type SideHistoryResponseMessage = z.infer<typeof SideHistoryResponseMessageSchema>;

/** Latest turn: the last follow-up, else the root turn. */
export function latestSideTurn(topic: BtwHistoryRecord): BtwHistoryTurn {
  const turn = topic.followUps?.at(-1);
  if (turn) return turn;
  return {
    question: topic.question,
    images: topic.images,
    answer: topic.answer,
    status: topic.status,
    createdAt: topic.createdAt,
    updatedAt: topic.updatedAt,
    error: topic.error,
  };
}

/** Whether a follow-up may be submitted: the latest turn must be complete. */
export function canFollowUp(topic: BtwHistoryRecord): boolean {
  return latestSideTurn(topic).status === "complete";
}

/** Read-only frozen conversation bundle for promotion into a full session. */
export const SideTransferRequestSchema = z.strictObject({
  type: z.literal("agent.side_question.transfer.request"),
  requestId: z.string().min(1),
  agentId: z.string(),
  recordId: z.string().min(1),
});
export type SideTransferRequest = z.infer<typeof SideTransferRequestSchema>;

export const SideTransferResponseMessageSchema = z.object({
  type: z.literal("agent.side_question.transfer.response"),
  payload: z.object({
    requestId: z.string(),
    snapshot: SideContextSnapshotSchema,
    topic: SideTopicSchema.extend({ followUps: z.array(BtwHistoryTurnSchema).optional() }),
  }),
});
export type SideTransferResponseMessage = z.infer<typeof SideTransferResponseMessageSchema>;

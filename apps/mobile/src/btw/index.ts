import type { BtwHistoryTurn, SideTopic } from "@ait/protocol/messages";
import { SIDE_QUESTION_REASONS } from "@ait/protocol/messages";
import type { TFunction } from "i18next";
import type { CommandOwnership } from "@/client-slash-commands";

/** A `/btw` submission recognised client-side; it never becomes a main prompt. */
export interface BtwCommand {
  /** Empty when the user asked for history only. */
  question: string;
  /** The command stays on the BTW route while the composer preserves its attachments. */
  attachmentsUnsupported?: boolean;
}

/** Decide ownership before opening the side panel or forwarding to the provider. */
export function routeBtwCommand(ownership: CommandOwnership): "side" | "native" | "uncertain" {
  if (ownership.kind === "legacy") return "side";
  if (ownership.kind === "unavailable") return "uncertain";
  const claims = ownership.commands.filter((entry) =>
    [entry.name, ...(entry.aliases ?? [])].some(
      (name) => name.replace(/:+$/u, "").toLowerCase() === "btw",
    ),
  );
  if (claims.length === 0) return "side";
  const nativeSources = new Set([
    "extension",
    "skill",
    "custom",
    "file",
    "project",
    "template",
    "prompt",
  ]);
  const allBuiltin = claims.every((claim) => claim.source === "builtin");
  return allBuiltin || claims.every((claim) => claim.source && nativeSources.has(claim.source))
    ? "native"
    : "uncertain";
}

export async function dispatchBtwCommand(input: {
  checkOwnership: () => Promise<CommandOwnership | null>;
  onSide: () => void;
  onNative: () => void;
  onUncertain: () => void;
}): Promise<void> {
  const ownership = await input.checkOwnership();
  if (!ownership) return;
  switch (routeBtwCommand(ownership)) {
    case "side":
      input.onSide();
      break;
    case "native":
      input.onNative();
      break;
    case "uncertain":
      input.onUncertain();
      break;
  }
}

/**
 * Detect `/btw` and its optional question.
 *
 * Absolute paths such as `/btw/file` are not commands, matching the native
 * slash-name split the provider adapters use.
 */
export function resolveBtwCommand(input: {
  text: string;
  hasAttachments: boolean;
}): BtwCommand | null {
  const trimmed = input.text.trim();
  if (!trimmed.startsWith("/btw")) return null;
  const rest = trimmed.slice("/btw".length);
  if (rest && !/^[\s:]/.test(rest)) return null;
  return {
    question: rest.replace(/^[\s:]+/, "").trim(),
    ...(input.hasAttachments ? { attachmentsUnsupported: true } : {}),
  };
}

/** Stable message for a rejected ask receipt, scoped to the BTW panel. */
export function describeBtwRejection(reason: string | undefined, t: TFunction): string {
  switch (reason) {
    case SIDE_QUESTION_REASONS.busy:
      return t("btw.busy");
    case SIDE_QUESTION_REASONS.unsupportedProvider:
      return t("btw.unsupportedProvider");
    case SIDE_QUESTION_REASONS.contextLimit:
      return t("btw.contextLimit");
    case SIDE_QUESTION_REASONS.invalidQuestion:
      return t("btw.invalidQuestion");
    case SIDE_QUESTION_REASONS.unknownTopic:
      return t("btw.unknownTopic");
    case SIDE_QUESTION_REASONS.topicNotSettled:
      return t("btw.topicNotSettled");
    default:
      return t("btw.askFailed");
  }
}

/**
 * Turns of a topic in order: the root turn, then its follow-ups.
 *
 * The root status keeps the previous turn's value while a follow-up runs, so
 * consumers read the last entry rather than the root status.
 */
export function btwTopicTurns(topic: SideTopic): BtwHistoryTurn[] {
  return [
    {
      question: topic.question,
      ...(topic.images ? { images: topic.images } : {}),
      answer: topic.answer,
      status: topic.status,
      createdAt: topic.createdAt,
      updatedAt: topic.updatedAt,
      ...(topic.error === undefined ? {} : { error: topic.error }),
    },
    ...(topic.followUps ?? []),
  ];
}

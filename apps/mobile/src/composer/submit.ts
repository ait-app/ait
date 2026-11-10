import { isDeliveredSendError } from "@/composer/submission/model";
import { i18n } from "@/i18n/i18next";

export type AgentInputSubmitResult = "noop" | "queued" | "submitted" | "delivered" | "failed";

/**
 * On web, clearing the input publishes to the draft store only after paint. Prefer the mounted
 * editor so a send that fails immediately does not read back the text it just cleared.
 */
export function readLiveDraftText(
  editor: { getText: () => string } | null,
  stored: { getSnapshot: () => string },
): string {
  return editor ? editor.getText() : stored.getSnapshot();
}

export interface AgentInputSubmitActionInput<TAttachment> {
  message: string;
  attachments: TAttachment[];
  hasExternalContent?: boolean;
  allowEmptySubmit?: boolean;
  submitBehavior?: "clear" | "preserve-and-lock";
  forceSend?: boolean;
  isAgentRunning: boolean;
  canSubmit: boolean;
  queueMessage: (input: { message: string; attachments: TAttachment[] }) => void;
  submitMessage: (input: { message: string; attachments: TAttachment[] }) => Promise<void>;
  clearDraft: (lifecycle: "sent" | "abandoned") => void;
  /**
   * Reads what the user entered after the composer was cleared for this send.
   * A failed send restores its message ahead of that content instead of
   * replacing it.
   */
  readCurrentDraft?: () => { text: string; attachments: TAttachment[] };
  setUserInput: (text: string) => void;
  setAttachments: (attachments: TAttachment[]) => void;
  setSendError: (message: string | null) => void;
  setIsProcessing: (isProcessing: boolean) => void;
  onSubmitError?: (error: unknown) => void;
  failedToSendMessage?: string;
}

/** Places restored message text ahead of whatever the composer holds now. */
export function joinRestoredText(restored: string, current: string): string {
  return [restored, current.trim() ? current : ""].filter(Boolean).join("\n\n");
}

export async function submitAgentInput<TAttachment>(
  input: AgentInputSubmitActionInput<TAttachment>,
): Promise<AgentInputSubmitResult> {
  const trimmedMessage = input.message.trim();
  // Leading whitespace can decide whether the provider treats this as a command at all.
  const message = trimmedMessage.startsWith("/") ? input.message : trimmedMessage;
  const attachments = input.attachments;
  const shouldClearOnSubmit = input.submitBehavior !== "preserve-and-lock";

  if (
    !trimmedMessage &&
    attachments.length === 0 &&
    !input.hasExternalContent &&
    !input.allowEmptySubmit
  ) {
    return "noop";
  }

  if (!input.canSubmit) {
    return "noop";
  }

  if (input.isAgentRunning && !input.forceSend) {
    input.queueMessage({ message, attachments });
    if (shouldClearOnSubmit) {
      input.setUserInput("");
      input.setAttachments([]);
    }
    return "queued";
  }

  // Clear immediately so the submitted timeline row and composer state stay in sync.
  if (shouldClearOnSubmit) {
    input.setUserInput("");
    input.setAttachments([]);
  }
  input.setSendError(null);
  input.setIsProcessing(true);

  try {
    await input.submitMessage({ message, attachments });
    // A cleared composer stays editable while the send is in flight; clearing
    // the draft again here would drop whatever the user entered meanwhile.
    if (!shouldClearOnSubmit) {
      input.clearDraft("sent");
    }
    input.setIsProcessing(false);
    return "submitted";
  } catch (error) {
    input.onSubmitError?.(error);
    // A delivered message is already in the timeline; restoring it invites a duplicate send.
    if (shouldClearOnSubmit && !isDeliveredSendError(error)) {
      const current = input.readCurrentDraft?.();
      input.setUserInput(joinRestoredText(message, current?.text ?? ""));
      input.setAttachments([...attachments, ...(current?.attachments ?? [])]);
    }
    input.setSendError(
      error instanceof Error
        ? error.message
        : (input.failedToSendMessage ?? i18n.t("composer.errors.failedToSend")),
    );
    input.setIsProcessing(false);
    return isDeliveredSendError(error) ? "delivered" : "failed";
  }
}

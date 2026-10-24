import { useCallback, useMemo, useReducer } from "react";
import { useTranslation } from "react-i18next";
import type { ComposerAttachment } from "@/attachments/types";
import {
  resolveComposerAttachmentSubmitFormat,
  splitComposerAttachmentsForSubmit,
} from "@/composer/attachments/submit";
import { isActiveCreateFlowForDraft, useCreateFlowStore } from "@/stores/create-flow-store";
import { handoffCreatedAgentMessageSubmission } from "@/composer/submission/writer";
import { useSessionStore } from "@/stores/session-store";
import {
  createUserMessage,
  generateMessageId,
  type StreamItem,
  type UserMessageImageAttachment,
} from "@/types/stream";
import type { AgentAttachment } from "@ait/protocol/messages";
import type { PendingMessageSubmission } from "@/composer/submission/model";

const EMPTY_STREAM_ITEMS: StreamItem[] = [];

interface CreateAttempt {
  clientMessageId: string;
  text: string;
  timestamp: Date;
  images?: UserMessageImageAttachment[];
  attachments?: AgentAttachment[];
}

type DraftAgentMachineState<TDraftAgent> =
  | { tag: "draft"; errorMessage: string }
  | { tag: "creating"; attempt: CreateAttempt; draftAgent: TDraftAgent };

type DraftAgentMachineEvent<TDraftAgent> =
  | { type: "DRAFT_SET_ERROR"; message: string }
  | { type: "SUBMIT"; attempt: CreateAttempt; draftAgent: TDraftAgent }
  | { type: "CREATE_FAILED"; message: string };

function assertNever(value: never): never {
  throw new Error(`Unhandled state: ${JSON.stringify(value)}`);
}

function reducer<TDraftAgent>(
  state: DraftAgentMachineState<TDraftAgent>,
  event: DraftAgentMachineEvent<TDraftAgent>,
): DraftAgentMachineState<TDraftAgent> {
  switch (event.type) {
    case "DRAFT_SET_ERROR": {
      if (state.tag !== "draft") {
        return state;
      }
      return { ...state, errorMessage: event.message };
    }
    case "SUBMIT": {
      return { tag: "creating", attempt: event.attempt, draftAgent: event.draftAgent };
    }
    case "CREATE_FAILED": {
      if (state.tag !== "creating") {
        return state;
      }
      return { tag: "draft", errorMessage: event.message };
    }
    default:
      return assertNever(event);
  }
}

function prepareCreateAttempt<TDraftAgent>(
  attempt: CreateAttempt,
  buildDraftAgent: (attempt: CreateAttempt) => TDraftAgent,
): DraftAgentMachineState<TDraftAgent> {
  try {
    return { tag: "creating", attempt, draftAgent: buildDraftAgent(attempt) };
  } catch (error) {
    return { tag: "draft", errorMessage: error instanceof Error ? error.message : String(error) };
  }
}

interface CreateRequestResult<TCreateResult> {
  agentId: string | null;
  result: TCreateResult;
}

interface SubmitContext {
  text: string;
  attachments: ComposerAttachment[];
  cwd: string;
}

interface CreateRequestContext {
  attempt: CreateAttempt;
  text: string;
  images?: UserMessageImageAttachment[];
  attachments?: AgentAttachment[];
  cwd: string;
}

interface UseDraftAgentCreateFlowOptions<TDraftAgent, TCreateResult> {
  draftId: string;
  getPendingServerId: () => string | null;
  initialAttempt?: CreateAttempt | null;
  validateBeforeCreate?: (ctx: { cwd: string }) => string | null;
  onBeforeSubmit?: (ctx: CreateRequestContext) => Promise<void> | void;
  onCreateStart?: () => void;
  createRequest: (ctx: CreateRequestContext) => Promise<CreateRequestResult<TCreateResult>>;
  buildDraftAgent: (attempt: CreateAttempt) => TDraftAgent;
  onCreateSuccess: (ctx: { result: TCreateResult; attempt: CreateAttempt }) => Promise<void> | void;
  onCreateError?: (error: Error) => void;
}

export function useDraftAgentCreateFlow<TDraftAgent, TCreateResult>({
  draftId,
  getPendingServerId,
  initialAttempt = null,
  validateBeforeCreate,
  onBeforeSubmit,
  onCreateStart,
  createRequest,
  buildDraftAgent,
  onCreateSuccess,
  onCreateError,
}: UseDraftAgentCreateFlowOptions<TDraftAgent, TCreateResult>) {
  const { t } = useTranslation();
  const [localMachine, dispatch] = useReducer(
    reducer<TDraftAgent>,
    initialAttempt,
    (attempt): DraftAgentMachineState<TDraftAgent> =>
      attempt
        ? prepareCreateAttempt(attempt, buildDraftAgent)
        : {
            tag: "draft",
            errorMessage: "",
          },
  );

  const pending = useCreateFlowStore((state) => state.pendingByDraftId[draftId]);
  // Remounts can precede model hydration. Rebuild the preview when its inputs
  // arrive, and observe the original request's failure through shared state.
  const machine = useMemo<DraftAgentMachineState<TDraftAgent>>(() => {
    if (pending?.lifecycle === "abandoned") {
      return { tag: "draft", errorMessage: pending.errorMessage ?? "" };
    }
    if (pending?.lifecycle === "active" && localMachine.tag === "draft" && initialAttempt) {
      return prepareCreateAttempt(initialAttempt, buildDraftAgent);
    }
    return localMachine;
  }, [pending, localMachine, initialAttempt, buildDraftAgent]);

  const setPendingCreateAttempt = useCreateFlowStore((state) => state.setPending);
  const updatePendingAgentId = useCreateFlowStore((state) => state.updateAgentId);
  const markPendingCreateLifecycle = useCreateFlowStore((state) => state.markLifecycle);
  const formErrorMessage = machine.tag === "draft" ? machine.errorMessage : "";
  const isSubmitting = machine.tag === "creating";

  const submittedStreamItems = useMemo<StreamItem[]>(() => {
    if (machine.tag !== "creating") {
      return EMPTY_STREAM_ITEMS;
    }

    if (
      !machine.attempt.text &&
      (!machine.attempt.images || machine.attempt.images.length === 0) &&
      (!machine.attempt.attachments || machine.attempt.attachments.length === 0)
    ) {
      return EMPTY_STREAM_ITEMS;
    }

    return [
      createUserMessage({
        clientMessageId: machine.attempt.clientMessageId,
        text: machine.attempt.text,
        timestamp: machine.attempt.timestamp,
        images: machine.attempt.images,
        attachments: machine.attempt.attachments,
      }),
    ];
  }, [machine]);
  const pendingMessageSubmissions = useMemo<readonly PendingMessageSubmission[]>(() => {
    if (machine.tag !== "creating" || submittedStreamItems.length === 0) return [];
    return [
      {
        clientMessageId: machine.attempt.clientMessageId,
      },
    ];
  }, [machine, submittedStreamItems]);

  const draftAgent = machine.tag === "creating" ? machine.draftAgent : null;
  const startCreateAttempt = useCallback(
    (attempt: CreateAttempt) => {
      const prepared = prepareCreateAttempt(attempt, buildDraftAgent);
      if (prepared.tag === "draft") {
        dispatch({ type: "DRAFT_SET_ERROR", message: prepared.errorMessage });
        throw new Error(prepared.errorMessage);
      }
      dispatch({ type: "SUBMIT", attempt, draftAgent: prepared.draftAgent });
    },
    [buildDraftAgent],
  );

  const runCreateAttempt = useCallback(
    async ({ attempt, cwd }: { attempt: CreateAttempt; cwd: string }) => {
      const pendingServerId = getPendingServerId();
      if (!pendingServerId) {
        const error = new Error(t("composer.errors.noHostSelected"));
        dispatch({ type: "DRAFT_SET_ERROR", message: error.message });
        throw error;
      }

      try {
        await onBeforeSubmit?.({
          attempt,
          text: attempt.text,
          images: attempt.images,
          attachments: attempt.attachments,
          cwd,
        });
        const createResult = await createRequest({
          attempt,
          text: attempt.text,
          images: attempt.images,
          attachments: attempt.attachments,
          cwd,
        });

        if (createResult.agentId) {
          updatePendingAgentId({ draftId, agentId: createResult.agentId });
          if (attempt.text || attempt.images?.length || attempt.attachments?.length) {
            handoffCreatedAgentMessageSubmission(
              pendingServerId,
              createResult.agentId,
              createUserMessage({
                clientMessageId: attempt.clientMessageId,
                text: attempt.text,
                timestamp: attempt.timestamp,
                images: attempt.images,
                attachments: attempt.attachments,
              }),
            );
          }
          markPendingCreateLifecycle({ draftId, lifecycle: "sent" });
        }

        await onCreateSuccess({ result: createResult.result, attempt });
        return createResult.result;
      } catch (error) {
        const resolved =
          error instanceof Error ? error : new Error(t("composer.errors.failedToCreateAgent"));
        dispatch({ type: "CREATE_FAILED", message: resolved.message });
        markPendingCreateLifecycle({
          draftId,
          lifecycle: "abandoned",
          errorMessage: resolved.message,
        });
        onCreateError?.(resolved);
        throw error;
      }
    },
    [
      createRequest,
      draftId,
      getPendingServerId,
      markPendingCreateLifecycle,
      onBeforeSubmit,
      onCreateError,
      onCreateSuccess,
      t,
      updatePendingAgentId,
    ],
  );

  const createAgent = useCallback(
    async ({ text, images, attachments, cwd }: Omit<CreateRequestContext, "attempt">) => {
      const existing = useCreateFlowStore.getState().pendingByDraftId[draftId];
      if (
        isSubmitting ||
        isActiveCreateFlowForDraft({ pending: existing, serverId: getPendingServerId(), draftId })
      ) {
        throw new Error(t("composer.errors.alreadyLoading"));
      }

      dispatch({ type: "DRAFT_SET_ERROR", message: "" });
      const pendingServerId = getPendingServerId();
      if (!pendingServerId) {
        const error = new Error(t("composer.errors.noHostSelected"));
        dispatch({ type: "DRAFT_SET_ERROR", message: error.message });
        throw error;
      }
      const validationError = validateBeforeCreate?.({ cwd });
      if (validationError) {
        dispatch({ type: "DRAFT_SET_ERROR", message: validationError });
        throw new Error(validationError);
      }

      const attempt: CreateAttempt = {
        clientMessageId: `${draftId}:${generateMessageId()}`,
        text,
        timestamp: new Date(),
        ...(images?.length ? { images } : {}),
        ...(attachments?.length ? { attachments } : {}),
      };
      startCreateAttempt(attempt);
      setPendingCreateAttempt({
        draftId,
        serverId: pendingServerId,
        agentId: null,
        clientMessageId: attempt.clientMessageId,
        text: attempt.text,
        timestamp: attempt.timestamp.getTime(),
        ...(attempt.images?.length ? { images: attempt.images } : {}),
        ...(attempt.attachments?.length ? { attachments: attempt.attachments } : {}),
      });

      onCreateStart?.();
      return await runCreateAttempt({ attempt, cwd });
    },
    [
      draftId,
      getPendingServerId,
      isSubmitting,
      onCreateStart,
      runCreateAttempt,
      setPendingCreateAttempt,
      startCreateAttempt,
      t,
      validateBeforeCreate,
    ],
  );

  const handleCreateFromInput = useCallback(
    async ({ text, attachments, cwd }: SubmitContext) => {
      const serverId = getPendingServerId();
      const supportsForgeSearch = serverId
        ? useSessionStore.getState().sessions[serverId]?.serverInfo?.features?.forgeSearch === true
        : false;
      const wirePayload = splitComposerAttachmentsForSubmit(attachments, {
        format: resolveComposerAttachmentSubmitFormat({
          supportsForgeAttachments: supportsForgeSearch,
        }),
      });
      const trimmedPrompt = text.trim();
      if (!trimmedPrompt && !wirePayload.images.length && !wirePayload.attachments.length) {
        const error = new Error(t("composer.errors.initialPromptRequired"));
        dispatch({ type: "DRAFT_SET_ERROR", message: error.message });
        throw error;
      }
      await createAgent({ text: trimmedPrompt, ...wirePayload, cwd });
    },
    [createAgent, getPendingServerId, t],
  );

  // Creating a session has no message requirement; sending a message does.
  const createEmptyAgent = useCallback(
    (cwd: string) => createAgent({ text: "", cwd }),
    [createAgent],
  );

  const continueCreateFromAttempt = useCallback(
    async ({ attempt, cwd }: { attempt: CreateAttempt; cwd: string }) => {
      if (!isSubmitting) {
        startCreateAttempt(attempt);
      }
      await runCreateAttempt({ attempt, cwd });
    },
    [isSubmitting, runCreateAttempt, startCreateAttempt],
  );

  return {
    machine,
    formErrorMessage,
    isSubmitting,
    submittedStreamItems,
    pendingMessageSubmissions,
    draftAgent,
    handleCreateFromInput,
    createEmptyAgent,
    continueCreateFromAttempt,
  };
}

export type { CreateAttempt as DraftCreateAttempt };

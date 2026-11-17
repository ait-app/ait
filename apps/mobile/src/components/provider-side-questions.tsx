import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import type { View } from "react-native";
import { useTranslation } from "react-i18next";
import type { SideQuestionsState } from "@ait/protocol/messages";
import { describeBtwRejection } from "@/btw";
import { useTransferSideQuestion } from "@/btw/use-transfer";
import type { BtwPanelRequest } from "@/btw/use-panel";
import { useComposerKeyboardScope } from "@/composer/keyboard-scope";
import { BtwFloatingSurface } from "@/components/btw-floating-surface";
import { BtwWindow } from "@/components/btw-window";
import { useHostRuntimeClient, useHostRuntimeIsConnected } from "@/runtime/host-runtime";
import { useBtwVisibilityStore } from "@/stores/btw-visibility-store";
import { useSessionStore } from "@/stores/session-store";
import { toErrorMessage } from "@/utils/error-messages";

export type BtwHistoryFailure = { kind: "unsupported" } | { kind: "retryable"; message: string };

/** Connection failures must remain retryable rather than masquerading as missing provider support. */
export function classifyBtwHistoryFailure(cause: unknown, supported: boolean): BtwHistoryFailure {
  const rawMessage = toErrorMessage(cause);
  const message =
    cause instanceof Error && cause.name === "DaemonRpcError"
      ? rawMessage.replace(/(?: requestType=\S+)?(?: code=\S+)?$/, "")
      : rawMessage;
  const code = (cause as { code?: unknown } | null)?.code;
  return !supported ||
    code === "UnsupportedCapability" ||
    code === "unsupported_capability" ||
    code === "not_implemented" ||
    message.includes("UnsupportedCapability")
    ? { kind: "unsupported" }
    : { kind: "retryable", message };
}

const EMPTY_STATE: SideQuestionsState = { records: [], omitted: 0 };

/** Open a small independent conversation without taking ownership of the main agent's input. */
export function BtwPanel({
  serverId,
  agentId,
  workspaceId,
  disabled,
  opened,
  request,
  onRequestConsumed,
  onOpenedChange,
  anchorRef,
  anchorLayoutVersion,
}: {
  serverId: string;
  agentId: string;
  workspaceId?: string | null;
  disabled: boolean;
  opened: boolean;
  request: BtwPanelRequest | null;
  onRequestConsumed: (key: number) => void;
  onOpenedChange: (opened: boolean) => void;
  anchorRef: RefObject<View | null>;
  anchorLayoutVersion?: number;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const isConnected = useHostRuntimeIsConnected(serverId);
  const sourceTitle = useSessionStore(
    (store) => store.sessions[serverId]?.agents.get(agentId)?.title,
  );
  const live = useSessionStore(
    (store) => store.sessions[serverId]?.agents.get(agentId)?.providerUi?.sideQuestions,
  );
  const owner = `${serverId}:${agentId}`;
  const hydrated = useBtwVisibilityStore((store) => store.hydrated);
  const ended = useBtwVisibilityStore((store) => store.ended.find(([key]) => key === owner)?.[1]);
  const { isPaneFocused } = useComposerKeyboardScope();
  const [history, setHistory] = useState<{
    state: SideQuestionsState;
    live: SideQuestionsState | undefined;
  } | null>(null);
  const [topicId, setTopicId] = useState<string | null>(null);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [pending, setPending] = useState<"ask" | "cancel" | "transfer" | null>(null);
  const [uncertain, setUncertain] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const inFlight = useRef(false);
  const historyVersion = useRef(0);
  const latestLive = useRef(live);
  const askedKey = useRef(0);
  const wasOpened = useRef(false);
  const transfer = useTransferSideQuestion({ serverId, agentId, workspaceId });
  const visible =
    isPaneFocused &&
    (opened || Boolean(hydrated && ended !== "*" && live?.reveal && live.reveal !== ended));
  const state = history && history.live === live ? history.state : (live ?? EMPTY_STATE);
  const topic = state.records.find((record) => record.id === topicId) ?? null;
  const draftKey = topicId ?? "new";
  const draft = drafts[draftKey] ?? "";
  const runningId = state.runningId;
  // The host persists an unresolved receipt before admitting an ask. While our
  // request is still in flight, that projection means sending, not a failed send.
  const acceptanceUncertain = pending !== "ask" && (uncertain || state.pendingAcceptance === true);
  const transferBlocked = disabled || acceptanceUncertain || !client;

  const invalidateHistory = useCallback(() => {
    historyVersion.current += 1;
    setLoading(false);
  }, []);

  // A history read can be newer than the last pushed agent snapshot. The next
  // snapshot becomes authoritative again, so a cancelled answer cannot reappear.
  useEffect(() => {
    latestLive.current = live;
    setLoading(false);
    setHistory(null);
    if (live?.pendingAcceptance !== undefined) setUncertain(live.pendingAcceptance);
  }, [live]);

  const refresh = useCallback(async () => {
    if (!client) return;
    const version = ++historyVersion.current;
    const snapshot = latestLive.current;
    setLoading(true);
    try {
      const next = await client.listSideQuestions({ agentId });
      if (version !== historyVersion.current) return;
      if (snapshot !== latestLive.current) return latestLive.current;
      setHistory({ state: next, live: snapshot });
      setError(null);
      setUncertain(next.pendingAcceptance === true);
      return next;
    } catch (cause) {
      if (version !== historyVersion.current || snapshot !== latestLive.current) return;
      const failure = classifyBtwHistoryFailure(cause, client.supportsSideQuestions());
      setError(failure.kind === "unsupported" ? t("btw.unsupportedHost") : failure.message);
    } finally {
      if (version === historyVersion.current) setLoading(false);
    }
  }, [agentId, client, t]);

  useEffect(() => {
    if (!visible || !client || !isConnected) {
      wasOpened.current = false;
      return;
    }
    if (!wasOpened.current) {
      wasOpened.current = true;
      // A command with a question loads history after admission. Reading it now
      // starts redundant provider work and can race with the pending receipt.
      if (request?.question) return;
      setTopicId(live?.runningId ?? live?.records[0]?.id ?? null);
      void refresh().then((next) => {
        if (next) setTopicId(next.runningId ?? next.records[0]?.id ?? null);
      });
    }
  }, [client, isConnected, live, visible, refresh, request?.question]);

  const setDraft = (text: string) => setDrafts((current) => ({ ...current, [draftKey]: text }));

  const ask = useCallback(
    async (question: string, recordId?: string) => {
      if (inFlight.current || !question.trim()) return;
      if (client && !client.supportsSideQuestions()) {
        setError(t("btw.unsupportedHost"));
        return;
      }
      if (runningId) {
        setTopicId(runningId);
        setError(t("btw.busy"));
        return;
      }
      if (!client || disabled || uncertain || state.pendingAcceptance || state.available === false)
        return;
      inFlight.current = true;
      invalidateHistory();
      setPending("ask");
      setError(null);
      try {
        const receipt = await client.askSideQuestion({
          agentId,
          question: question.trim(),
          recordId,
        });
        if (receipt.receipt === "rejected") {
          setError(describeBtwRejection(receipt.reason, t));
          return;
        }
        if (receipt.recordId) setTopicId(receipt.recordId);
        setDrafts((current) => {
          const key = recordId ?? "new";
          const remaining = current[key] === question ? "" : (current[key] ?? "");
          return {
            ...current,
            [key]: "",
            ...(receipt.recordId ? { [receipt.recordId]: remaining } : { [key]: remaining }),
          };
        });
        if (receipt.receipt === "uncertain") {
          setUncertain(true);
        } else {
          setUncertain(false);
          await refresh();
        }
      } catch (cause) {
        const failure = classifyBtwHistoryFailure(cause, client.supportsSideQuestions());
        setUncertain(failure.kind !== "unsupported");
        setError(failure.kind === "unsupported" ? t("btw.unsupportedHost") : failure.message);
      } finally {
        inFlight.current = false;
        setPending(null);
      }
    },
    [
      agentId,
      client,
      disabled,
      invalidateHistory,
      refresh,
      runningId,
      state.available,
      state.pendingAcceptance,
      t,
      uncertain,
    ],
  );

  useEffect(() => {
    if (
      !visible ||
      !client ||
      !isConnected ||
      !request?.question ||
      request.owner !== owner ||
      request.key === askedKey.current
    )
      return;
    askedKey.current = request.key;
    onRequestConsumed(request.key);
    setTopicId(null);
    setDrafts((current) => ({ ...current, new: request.question }));
    void ask(request.question);
  }, [ask, client, isConnected, onRequestConsumed, owner, request, visible]);

  const hide = () => {
    useBtwVisibilityStore
      .getState()
      .end(owner, state.reveal ?? topic?.id ?? runningId ?? state.records[0]?.id ?? "*");
    onOpenedChange(false);
  };

  const cancel = async (close: boolean) => {
    if (uncertain || state.pendingAcceptance) return;
    if (inFlight.current || (runningId && (!client || disabled))) return;
    if (!runningId) {
      if (close) hide();
      return;
    }
    inFlight.current = true;
    invalidateHistory();
    setPending("cancel");
    setError(null);
    try {
      const cancelled = await client!.cancelSideQuestion({ agentId, recordId: runningId });
      if (!cancelled) {
        const next = await refresh();
        if (!next || next.runningId) {
          setError(t("btw.cancelFailed"));
          return;
        }
      }
      if (close) hide();
      else await refresh();
    } catch (cause) {
      const failure = classifyBtwHistoryFailure(cause, client!.supportsSideQuestions());
      setError(failure.kind === "unsupported" ? t("btw.unsupportedHost") : failure.message);
    } finally {
      inFlight.current = false;
      setPending(null);
    }
  };

  const expand = async () => {
    if (!topic || !workspaceId || transferBlocked || inFlight.current || runningId) return;
    inFlight.current = true;
    invalidateHistory();
    setPending("transfer");
    setError(null);
    try {
      await transfer(topic.id);
      hide();
    } catch (cause) {
      const failure = classifyBtwHistoryFailure(cause, client!.supportsSideQuestions());
      setError(failure.kind === "unsupported" ? t("btw.unsupportedHost") : failure.message);
    } finally {
      inFlight.current = false;
      setPending(null);
    }
  };

  return (
    <BtwFloatingSurface
      anchorRef={anchorRef}
      anchorLayoutVersion={anchorLayoutVersion}
      visible={visible}
      onClose={() => void cancel(true)}
      onSubmit={() => void ask(draft, topicId ?? undefined)}
    >
      <BtwWindow
        sourceTitle={sourceTitle ?? t("btw.sourceFallback")}
        state={state}
        topic={topic}
        draft={draft}
        error={
          error ??
          state.unavailableReason ??
          (acceptanceUncertain
            ? t("btw.uncertain")
            : pending === "ask" && state.pendingAcceptance
              ? null
              : (state.error ?? null))
        }
        loading={loading && !history && !live}
        pending={pending !== null}
        disabled={
          disabled ||
          acceptanceUncertain ||
          state.available === false ||
          !client ||
          client.supportsSideQuestions() === false
        }
        historyOpen={historyOpen}
        canTransfer={Boolean(topic && workspaceId && !transferBlocked)}
        closeBlocked={acceptanceUncertain}
        onDraftChange={setDraft}
        onSubmit={() => void ask(draft, topicId ?? undefined)}
        onCancel={() => void cancel(false)}
        onClose={() => void cancel(true)}
        onTransfer={() => void expand()}
        onNew={() => {
          setTopicId(null);
          setHistoryOpen(false);
        }}
        onHistoryToggle={() => setHistoryOpen((current) => !current)}
        onSelectTopic={(id) => {
          setTopicId(id);
          setHistoryOpen(false);
        }}
        onRetry={() => void refresh()}
      />
    </BtwFloatingSurface>
  );
}

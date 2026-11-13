import { useCallback, useEffect, useRef, useState } from "react";
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

export type BtwHistoryFailure = { kind: "unsupported" } | { kind: "retryable"; message: string };

/** Connection failures must remain retryable rather than masquerading as missing provider support. */
export function classifyBtwHistoryFailure(cause: unknown, supported: boolean): BtwHistoryFailure {
  const rawMessage = cause instanceof Error ? cause.message : String(cause);
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
}: {
  serverId: string;
  agentId: string;
  workspaceId?: string | null;
  disabled: boolean;
  opened: boolean;
  request: BtwPanelRequest | null;
  onRequestConsumed: (key: number) => void;
  onOpenedChange: (opened: boolean) => void;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const isConnected = useHostRuntimeIsConnected(serverId);
  const agent = useSessionStore((store) => store.sessions[serverId]?.agents.get(agentId));
  const live = agent?.providerUi?.sideQuestions;
  const owner = `${serverId}:${agentId}`;
  const hydrated = useBtwVisibilityStore((store) => store.hydrated);
  const ended = useBtwVisibilityStore((store) => store.ended.find(([key]) => key === owner)?.[1]);
  const { isPaneFocused } = useComposerKeyboardScope();
  const [history, setHistory] = useState<SideQuestionsState | null>(null);
  const [topicId, setTopicId] = useState<string | null>(null);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [pending, setPending] = useState(false);
  const [uncertain, setUncertain] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const inFlight = useRef(false);
  const askedKey = useRef(0);
  const wasOpened = useRef(false);
  const transfer = useTransferSideQuestion({ serverId, agentId, workspaceId });
  const visible =
    isPaneFocused &&
    (opened || Boolean(hydrated && ended !== "*" && live?.reveal && live.reveal !== ended));
  const state = history ?? live ?? EMPTY_STATE;
  const topic = state.records.find((record) => record.id === topicId) ?? null;
  const draftKey = topicId ?? "new";
  const draft = drafts[draftKey] ?? "";
  const runningId = state.runningId;
  const transferBlocked = disabled || uncertain || state.pendingAcceptance === true || !client;

  // A history read can be newer than the last pushed agent snapshot. The next
  // snapshot becomes authoritative again, so a cancelled answer cannot reappear.
  useEffect(() => {
    setHistory(null);
    if (live?.pendingAcceptance !== undefined) setUncertain(live.pendingAcceptance);
  }, [live]);

  const refresh = useCallback(async () => {
    if (!client) return;
    setLoading(true);
    try {
      const next = await client.listSideQuestions({ agentId });
      setHistory(next);
      setError(next.pendingAcceptance ? t("btw.uncertain") : null);
      setUncertain(next.pendingAcceptance === true);
      return next;
    } catch (cause) {
      const failure = classifyBtwHistoryFailure(cause, client.supportsSideQuestions());
      setError(failure.kind === "unsupported" ? t("btw.unsupportedHost") : failure.message);
    } finally {
      setLoading(false);
    }
  }, [agentId, client, t]);

  useEffect(() => {
    if (!visible || !client || !isConnected) {
      wasOpened.current = false;
      return;
    }
    if (!wasOpened.current) {
      wasOpened.current = true;
      void refresh().then((next) => {
        if (!request?.question) setTopicId(next?.runningId ?? next?.records[0]?.id ?? null);
      });
    }
  }, [client, isConnected, visible, refresh, request?.question]);

  const setDraft = (text: string) => setDrafts((current) => ({ ...current, [draftKey]: text }));

  const ask = useCallback(
    async (question: string, recordId?: string) => {
      if (client && !client.supportsSideQuestions()) {
        setError(t("btw.unsupportedHost"));
        return;
      }
      if (runningId) {
        setTopicId(runningId);
        setError(t("btw.busy"));
        return;
      }
      if (
        !client ||
        disabled ||
        uncertain ||
        state.pendingAcceptance ||
        state.available === false ||
        inFlight.current ||
        !question.trim()
      )
        return;
      inFlight.current = true;
      setPending(true);
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
          setError(t("btw.uncertain"));
        } else {
          await refresh();
        }
      } catch (cause) {
        const failure = classifyBtwHistoryFailure(cause, client.supportsSideQuestions());
        setUncertain(failure.kind !== "unsupported");
        setError(failure.kind === "unsupported" ? t("btw.unsupportedHost") : failure.message);
      } finally {
        inFlight.current = false;
        setPending(false);
      }
    },
    [
      agentId,
      client,
      disabled,
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
    setPending(true);
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
      setPending(false);
    }
  };

  const expand = async () => {
    if (!topic || !workspaceId || transferBlocked || inFlight.current || runningId) return;
    inFlight.current = true;
    setPending(true);
    setError(null);
    try {
      await transfer(topic.id);
      hide();
    } catch (cause) {
      const failure = classifyBtwHistoryFailure(cause, client!.supportsSideQuestions());
      setError(failure.kind === "unsupported" ? t("btw.unsupportedHost") : failure.message);
    } finally {
      inFlight.current = false;
      setPending(false);
    }
  };

  return (
    <BtwFloatingSurface
      visible={visible}
      onClose={() => void cancel(true)}
      onSubmit={() => void ask(draft, topicId ?? undefined)}
    >
      <BtwWindow
        sourceTitle={agent?.title ?? t("btw.sourceFallback")}
        state={state}
        topic={topic}
        draft={draft}
        error={error ?? state.unavailableReason ?? state.error ?? null}
        loading={loading && !history && !live}
        pending={pending}
        disabled={
          disabled ||
          uncertain ||
          state.pendingAcceptance === true ||
          state.available === false ||
          !client ||
          client.supportsSideQuestions() === false
        }
        historyOpen={historyOpen}
        canTransfer={Boolean(topic && workspaceId && !transferBlocked)}
        closeBlocked={uncertain || state.pendingAcceptance === true}
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

import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { FlatList, Text, View } from "react-native";
import { randomUUID } from "expo-crypto";
import { useTranslation } from "react-i18next";
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { RotateCcw, Square } from "lucide-react-native";
import { withUnistyles } from "react-native-unistyles";
import type { BotTurn, BotView } from "@ait/protocol/bot/types";
import { AssistantMessage, AssistantTurnFooter, UserMessage } from "@/components/message";
import { SyncedLoader } from "@/components/synced-loader";
import { Button } from "@/components/ui/button";
import { mutedIconColorMapping } from "@/components/ui/icon-button-chrome";
import { ToolbarButton } from "@/components/ui/pane-content-toolbar";
import { ComposerDock } from "@/composer/dock";
import { MessageInput, type MessageInputRef } from "@/composer/input/input";
import { resolveComposerTrackTailClearance } from "@/composer/pill-styles";
import { ComposerTrackBar } from "@/composer/tracks";
import type { MessagePayload, TextReplacement } from "@/composer/types";
import { useIsCompactFormFactor } from "@/constants/layout";
import { canManageBackgroundTasks } from "@/hosts/background-task-access";
import { useSettings } from "@/hooks/use-settings";
import { usePaneContext } from "@/panels/pane-context";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { useSubagentOpensInSheet } from "@/subagents/opens-in-sheet";
import type { PaseoSubagentRow } from "@/subagents/select";
import { SubagentsTrack } from "@/subagents/track";
import type { Theme } from "@/styles/theme";
import { TURN_LIVENESS_IDLE, type TurnLiveness } from "@/timeline/turn-liveness";
import { confirmDialog } from "@/utils/confirm-dialog";
import { buildWorkspaceTabPersistenceKey, type WorkspaceTabTarget } from "@/workspace-tabs/model";
import { openPreferredWorkspaceTarget } from "@/workspace-tabs/open-beside";
import { BotSilentNote, BotWakeHeader, visibleBotMessages } from "./bot-wake";
import { BotLoadingSpinner } from "./loading";
import { botKeys, isBotTurnPending, useBotHistory } from "./queries";
import { closeBotSessionTab } from "./session-tab";
import { botChatStyles as chatStyles, botStyles as styles } from "./styles";

const ThemedSyncedLoader = withUnistyles(SyncedLoader);
const ThemedRotateCcw = withUnistyles(RotateCcw);
const ThemedSquare = withUnistyles(Square);
const foregroundColorMapping = (theme: Theme) => ({ color: theme.colors.foreground });
const INITIAL_TEXT: TextReplacement = { kind: "initial", key: "bot-chat", text: "" };
const NO_ATTACHMENTS: never[] = [];
const NOOP = () => undefined;
const RUNNING_TURN: TurnLiveness = {
  phase: "open",
  turnId: null,
  startedAt: null,
  cancellationRequestId: null,
};

/**
 * Opens a Workspace target from the Bot chat beside it, as a session page opens its children.
 * `pin` keeps a tab whose Agent the Agent directory does not list, such as the Bot's own session.
 */
function useOpenFromBot(serverId: string, workspaceId: string) {
  const { tabId } = usePaneContext();
  const openInSidePane = useSettings((settings) => settings.openInSidePane);
  const workspaceKey = buildWorkspaceTabPersistenceKey({ serverId, workspaceId });
  return useCallback(
    (target: WorkspaceTabTarget, pin = false) => {
      if (!workspaceKey) return;
      openPreferredWorkspaceTarget({
        workspaceKey,
        target,
        source: "subagents",
        preferences: openInSidePane,
        parentTabId: tabId,
        ...(pin ? { pin } : {}),
      });
    },
    [openInSidePane, tabId, workspaceKey],
  );
}

/**
 * One exchange: the prompt in a user bubble, or for a wake turn the notifications that started
 * it, then the Bot's reply as an assistant message.
 */
function BotExchange({
  turn,
  serverId,
  workspaceId,
  cwd,
  onOpen,
}: {
  turn: BotTurn;
  serverId: string;
  workspaceId: string;
  cwd: string;
  onOpen: (target: WorkspaceTabTarget) => void;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const createdAt = useMemo(() => new Date(turn.createdAt), [turn.createdAt]);
  const pending = isBotTurnPending(turn);
  const messages = useMemo(() => visibleBotMessages(turn, pending), [turn, pending]);
  const getReply = useCallback(
    () => messages.map((message) => message.text).join("\n\n"),
    [messages],
  );
  return (
    <View testID={`bot-turn-${turn.id}`}>
      {turn.wake ? (
        <BotWakeHeader turn={turn} workspaceId={workspaceId} onOpen={onOpen} />
      ) : (
        <UserMessage
          serverId={serverId}
          messageId={turn.messageId}
          message={turn.text}
          timestamp={createdAt.getTime()}
          client={client}
        />
      )}
      {messages.map((message) => (
        <AssistantMessage
          key={message.id}
          message={message.text}
          timestamp={createdAt.getTime()}
          serverId={serverId}
          client={client}
          workspaceRoot={cwd}
          phase={message.status === "streaming" && pending ? "streaming" : "complete"}
          occurrenceKey={`${turn.id}:${message.id}`}
        />
      ))}
      {turn.wake && turn.silent ? <BotSilentNote turnId={turn.id} /> : null}
      {pending ? (
        <View style={chatStyles.turnStatus} testID={`bot-turn-status-${turn.id}`}>
          <ThemedSyncedLoader size={14} uniProps={foregroundColorMapping} />
          <Text style={chatStyles.turnStatusText}>{t(`bots.status.${turn.status}`)}</Text>
        </View>
      ) : turn.status !== "completed" ? (
        <Text style={chatStyles.turnStatusText}>{t(`bots.status.${turn.status}`)}</Text>
      ) : null}
      {turn.error ? (
        <Text selectable style={styles.error}>
          {turn.error}
        </Text>
      ) : null}
      {messages.length > 0 && !pending ? (
        <View style={chatStyles.turnFooter}>
          <AssistantTurnFooter getContent={getReply} />
        </View>
      ) : null}
    </View>
  );
}

/** Lists the Bot's own session in the subagent pill, as a session page lists its children. */
function BotSessionTrack({
  serverId,
  workspaceId,
  sessionAgentId,
  provider,
  busy,
}: {
  serverId: string;
  workspaceId: string;
  sessionAgentId: string;
  provider: string;
  busy: boolean;
}) {
  const { t } = useTranslation();
  const workspaceKey = buildWorkspaceTabPersistenceKey({ serverId, workspaceId });
  const opensInSheet = useSubagentOpensInSheet(workspaceKey);
  const open = useOpenFromBot(serverId, workspaceId);
  const title = t("bots.session");
  const rows = useMemo<PaseoSubagentRow[]>(
    () => [
      {
        kind: "paseo",
        id: sessionAgentId,
        provider,
        title,
        description: null,
        subtitle: null,
        canArchive: false,
        canDetach: false,
        status: busy ? "running" : "idle",
        turn: busy ? RUNNING_TURN : TURN_LIVENESS_IDLE,
        requiresAttention: false,
        createdAt: new Date(0),
      },
    ],
    [busy, provider, sessionAgentId, title],
  );
  const openSession = useCallback(
    // The session never joins the Agent directory, which would otherwise prune its tab.
    (agentId: string) => open({ kind: "agent", agentId }, true),
    [open],
  );
  return (
    <ComposerTrackBar>
      <SubagentsTrack
        serverId={serverId}
        rows={rows}
        onOpenSubagent={openSession}
        onOpenProviderSubagent={NOOP}
        opensInSheet={opensInSheet}
        onArchiveSubagent={NOOP}
      />
    </ComposerTrackBar>
  );
}

export function BotChat({
  serverId,
  workspaceId,
  cwd,
  bot,
  interactive,
}: {
  serverId: string;
  workspaceId: string;
  cwd: string;
  bot: BotView;
  interactive: boolean;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const readOnly = !canManageBackgroundTasks(serverId);
  const compact = useIsCompactFormFactor();
  const queryClient = useQueryClient();
  const [text, setText] = useState("");
  const input = useRef<MessageInputRef>(null);
  const attempt = useRef<{ text: string; conversationId: string; messageId: string } | null>(null);
  const list = useRef<FlatList<BotTurn>>(null);
  const atBottom = useRef(true);
  // Only an active Bot accepts `open`; otherwise read the builtin conversation if it exists.
  // The access mode is part of the key, so resuming opens the conversation without a remount.
  const writable = !readOnly && bot.state.kind === "active";
  const conversationKey = [
    ...botKeys.conversation(serverId, workspaceId, bot.builtinChannelId),
    writable ? "open" : "read",
  ];
  const conversation = useQuery({
    queryKey: conversationKey,
    enabled: interactive && Boolean(client),
    staleTime: 0,
    placeholderData: keepPreviousData,
    queryFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      if (!writable) {
        const { conversations } = await client.botRequest("bot.conversation.list.request", {
          workspaceId,
        });
        return conversations.find((item) => item.channelId === bot.builtinChannelId) ?? null;
      }
      return (await client.botRequest("bot.conversation.open.request", { workspaceId }))
        .conversation;
    },
  });
  const history = useBotHistory(serverId, workspaceId, conversation.data?.id, interactive);
  const openTarget = useOpenFromBot(serverId, workspaceId);
  const refresh = () => {
    void history.refetch();
  };
  const send = useMutation({
    mutationFn: async (message: string) => {
      if (readOnly) throw new Error(t("bots.requestFailed"));
      if (!client || !conversation.data) throw new Error(t("bots.disconnected"));
      if (new TextEncoder().encode(message).length > 16 * 1024) throw new Error(t("bots.tooLong"));
      // Keep the receipt identity after a timeout so retrying cannot duplicate input.
      if (
        attempt.current?.text !== message ||
        attempt.current?.conversationId !== conversation.data.id
      ) {
        attempt.current = {
          text: message,
          conversationId: conversation.data.id,
          messageId: randomUUID(),
        };
      }
      return client.botRequest("bot.message.send.request", { workspaceId, ...attempt.current });
    },
    onSuccess: ({ turn }) => {
      queryClient.setQueryData<BotTurn[]>(
        botKeys.history(serverId, workspaceId, turn.conversationId),
        (cached = []) =>
          [...cached.filter((item) => item.id !== turn.id), turn].sort(
            (left, right) => left.sequence - right.sequence,
          ),
      );
      input.current?.replaceText("");
      setText("");
      attempt.current = null;
      atBottom.current = true;
      refresh();
    },
  });
  const reset = useMutation({
    mutationFn: async () => {
      if (readOnly) throw new Error(t("bots.requestFailed"));
      if (!client || !conversation.data) throw new Error(t("bots.disconnected"));
      const confirmed = await confirmDialog({
        title: t("bots.reset"),
        message: t("bots.resetConfirm"),
        confirmLabel: t("bots.reset"),
        cancelLabel: t("common.actions.cancel"),
      });
      if (!confirmed) return null;
      return (
        await client.botRequest("bot.conversation.reset.request", {
          workspaceId,
          conversationId: conversation.data.id,
        })
      ).conversation;
    },
    onSuccess: (updated) => {
      if (updated) {
        queryClient.setQueryData(conversationKey, updated);
        refresh();
      }
    },
  });
  const resume = useMutation({
    mutationFn: async () => {
      if (readOnly) throw new Error(t("bots.requestFailed"));
      if (!client) throw new Error(t("bots.disconnected"));
      return (await client.botRequest("bot.resume.request", { workspaceId })).bot;
    },
    onSuccess: (resumed) => {
      queryClient.setQueryData(botKeys.bot(serverId, workspaceId), resumed);
    },
  });
  const turns = history.data ?? [];
  const pendingTurn = turns.find(isBotTurnPending);
  const busy = Boolean(pendingTurn);
  const cancel = useMutation({
    mutationFn: async (turnId: string) => {
      if (readOnly) throw new Error(t("bots.requestFailed"));
      if (!client) throw new Error(t("bots.disconnected"));
      await client.botRequest("bot.turn.cancel.request", { workspaceId, turnId });
    },
    onSuccess: refresh,
  });
  const pausedMessage =
    bot.state.kind !== "paused"
      ? null
      : bot.state.reason === "directory_missing"
        ? t("bots.directoryMissing")
        : t("bots.paused");
  const disabled =
    !interactive ||
    !writable ||
    !conversation.data ||
    conversation.isPlaceholderData ||
    reset.isPending;
  // A turn creates the hidden session when it starts running, and a reset rotates it, so only a
  // started turn of the current generation proves the published session exists.
  const sessionStarted = turns.some(
    (turn) => turn.generation === conversation.data?.generation && turn.status !== "queued",
  );
  const sessionAgentId = sessionStarted ? conversation.data?.sessionAgentId : undefined;
  // A reset retires the published session; its open tab could only show a load error.
  const publishedSession = conversation.data?.sessionAgentId;
  const previousSession = useRef(publishedSession);
  useEffect(() => {
    if (!publishedSession) return;
    if (previousSession.current && previousSession.current !== publishedSession) {
      closeBotSessionTab(serverId, workspaceId, previousSession.current);
    }
    previousSession.current = publishedSession;
  }, [publishedSession, serverId, workspaceId]);
  const handleSubmit = (payload: MessagePayload) => {
    if (payload.text.trim()) send.mutate(payload.text);
  };
  const errors = [
    conversation.error,
    history.error,
    send.error,
    reset.error,
    resume.error,
    cancel.error,
  ].filter(Boolean);
  const paddingBottom = sessionAgentId ? resolveComposerTrackTailClearance(compact) : undefined;
  const content = (
    <View style={chatStyles.transcript}>
      {conversation.isPending && !conversation.data && bot.state.kind !== "retired" ? (
        <BotLoadingSpinner />
      ) : null}
      <FlatList
        ref={list}
        data={turns}
        keyExtractor={(turn) => turn.id}
        contentContainerStyle={[chatStyles.listContent, paddingBottom ? { paddingBottom } : null]}
        renderItem={({ item }) => (
          <BotExchange
            turn={item}
            serverId={serverId}
            workspaceId={workspaceId}
            cwd={cwd}
            onOpen={openTarget}
          />
        )}
        ListEmptyComponent={
          <View style={chatStyles.emptyChat}>
            <Text style={styles.hint}>{t("bots.emptyChat")}</Text>
          </View>
        }
        onScroll={({ nativeEvent }) => {
          atBottom.current =
            nativeEvent.contentSize.height -
              nativeEvent.layoutMeasurement.height -
              nativeEvent.contentOffset.y <
            80;
        }}
        scrollEventThrottle={100}
        onContentSizeChange={() => {
          if (atBottom.current) list.current?.scrollToEnd({ animated: false });
        }}
      />
      {sessionAgentId ? (
        <BotSessionTrack
          serverId={serverId}
          workspaceId={workspaceId}
          sessionAgentId={sessionAgentId}
          provider={bot.execution.provider}
          busy={busy}
        />
      ) : null}
    </View>
  );
  const composer = (
    <View style={chatStyles.composerRail}>
      {pausedMessage ? (
        <View style={chatStyles.notice}>
          <Text style={[styles.hint, styles.grow]}>{pausedMessage}</Text>
          {!readOnly ? (
            <Button
              variant="outline"
              size="sm"
              disabled={resume.isPending}
              onPress={() => resume.mutate()}
              testID="bot-resume"
            >
              {t("bots.resume")}
            </Button>
          ) : null}
        </View>
      ) : null}
      {errors.map((error, index) => (
        <Text key={index} style={styles.error} accessibilityRole="alert">
          {error?.message}
        </Text>
      ))}
      {!readOnly ? (
        <MessageInput
          ref={input}
          value=""
          onChangeText={setText}
          onSubmit={handleSubmit}
          submitButtonTestID="bot-message-send"
          isSubmitDisabled={disabled || !text.trim() || send.isPending}
          isSubmitLoading={send.isPending}
          // The text stays until the host accepts it, so a failed send can be retried as is.
          preserveHeightOnSubmit
          attachments={NO_ATTACHMENTS}
          cwd={cwd}
          attachmentMenuItems={NO_ATTACHMENTS}
          client={client}
          placeholder={t("bots.message")}
          disabled={disabled || send.isPending}
          isAgentRunning={busy}
          // A turn sent while another runs waits its turn on the host, so sending never blocks.
          defaultSendBehavior="interrupt"
          textReplacement={INITIAL_TEXT}
          leftContent={
            <Text style={styles.hint} numberOfLines={1} testID="bot-execution">
              {bot.execution.model
                ? `${bot.execution.provider} · ${bot.execution.model}`
                : bot.execution.provider}
            </Text>
          }
          beforeVoiceContent={
            <ToolbarButton
              label={t("bots.reset")}
              onPress={() => reset.mutate()}
              disabled={disabled || busy || send.isPending}
              testID="bot-reset"
            >
              <ThemedRotateCcw size={14} uniProps={mutedIconColorMapping} />
            </ToolbarButton>
          }
          activeActionContent={
            pendingTurn ? (
              <ToolbarButton
                label={t("bots.cancel")}
                onPress={() => cancel.mutate(pendingTurn.id)}
                disabled={cancel.isPending}
                testID="bot-cancel"
              >
                <ThemedSquare size={14} uniProps={mutedIconColorMapping} />
              </ToolbarButton>
            ) : null
          }
        />
      ) : null}
    </View>
  );
  return (
    <View style={styles.root} testID="bot-chat">
      <ComposerDock>{[content, composer]}</ComposerDock>
    </View>
  );
}

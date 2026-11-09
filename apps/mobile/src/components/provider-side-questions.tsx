import { memo, useCallback, useEffect, useRef, useState } from "react";
import { ScrollView, Text, TextInput, View } from "react-native";
import { ChevronDown, ChevronRight, X } from "lucide-react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import type { ProviderUiState } from "@ait/protocol/messages";
import { Button } from "@/components/ui/button";
import { StatusBadge } from "@/components/ui/status-badge";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { MarkdownRenderer } from "@/components/markdown/renderer";

type State = NonNullable<ProviderUiState["sideQuestions"]>;
type Topic = State["records"][number];

export function ProviderSideQuestions({
  state,
  serverId,
  agentId,
  disabled,
}: {
  state: State;
  serverId: string;
  agentId: string;
  disabled: boolean;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const [expanded, setExpanded] = useState(false);
  const [draft, setDraft] = useState("");
  const [topicId, setTopicId] = useState<string | null>(state.runningId);
  useEffect(() => {
    if (state.runningId) setTopicId(state.runningId);
  }, [state.runningId]);
  const selectedTopic =
    state.records.find((record) => record.id === topicId) ??
    state.records[state.records.length - 1];
  const [replyTo, setReplyTo] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const seenReveal = useRef(state.reveal);
  const seenRunningId = useRef(state.runningId);
  useEffect(() => {
    if (state.reveal) setExpanded(true);
  }, [state.reveal]);
  // Dismissing hides the panel locally only; native history stays untouched.
  useEffect(() => {
    if (seenReveal.current !== state.reveal) {
      seenReveal.current = state.reveal;
      if (state.reveal) setDismissed(false);
    }
    if (seenRunningId.current !== state.runningId) {
      seenRunningId.current = state.runningId;
      if (state.runningId) setDismissed(false);
    }
  }, [state.reveal, state.runningId]);
  const run = useCallback(
    async (action: string, arguments_: Record<string, string>) => {
      if (!client) throw new Error(t("providerControl.sideQuestions.disconnected"));
      return client.runNativeAction(agentId, action, arguments_);
    },
    [client, agentId, t],
  );
  const send = async () => {
    if (disabled || sending || state.runningId || !draft.trim()) return;
    const submitted = draft;
    setSending(true);
    setError(null);
    try {
      await run("btw", { question: submitted.trim(), ...(replyTo ? { recordId: replyTo } : {}) });
      setDraft((current) => (current === submitted ? "" : current));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setSending(false);
    }
  };
  const followUp = useCallback((id: string) => setReplyTo(id), []);
  if (dismissed) return null;
  if (!state.records.length && !state.error && !state.reveal) return null;
  return (
    <View style={styles.panel} testID="provider-side-questions">
      <View style={styles.header}>
        <Button
          variant="ghost"
          size="sm"
          style={styles.headerToggle}
          leftIcon={expanded ? ChevronDown : ChevronRight}
          accessibilityState={{ expanded }}
          onPress={() => setExpanded(!expanded)}
          testID="side-question-toggle"
        >
          <Text style={styles.title}>{t("providerControl.sideQuestions.title")}</Text>
        </Button>
        <Button
          variant="ghost"
          size="sm"
          leftIcon={X}
          accessibilityLabel={t("providerControl.sideQuestions.close")}
          onPress={() => setDismissed(true)}
          testID="side-question-close"
        />
      </View>
      {state.runningId ? (
        <Text style={styles.caption}>{t("providerControl.sideQuestions.closeWhileRunning")}</Text>
      ) : null}
      {expanded ? (
        <>
          {state.records.length > 1 ? (
            <ScrollView horizontal style={styles.topicPicker} contentContainerStyle={styles.row}>
              {state.records.map((record) => (
                <Button
                  key={record.id}
                  size="xs"
                  variant={record.id === selectedTopic?.id ? "secondary" : "ghost"}
                  onPress={() => setTopicId(record.id)}
                >
                  <Text style={styles.topicLabel} numberOfLines={1}>
                    {record.question}
                  </Text>
                </Button>
              ))}
            </ScrollView>
          ) : null}
          <View style={styles.list}>
            {!state.records.length ? (
              <Text style={styles.caption}>{t("providerControl.sideQuestions.empty")}</Text>
            ) : null}
            {(selectedTopic ? [selectedTopic] : []).map((record) => (
              <SideTopic
                key={record.id}
                record={record}
                running={state.runningId === record.id}
                disabled={disabled}
                run={run}
                onFollowUp={followUp}
              />
            ))}
            {state.omitted ? (
              <Text style={styles.caption}>
                {t("providerControl.sideQuestions.older", { count: state.omitted })}
              </Text>
            ) : null}
          </View>
          {replyTo ? (
            <View style={styles.row}>
              <Text style={styles.caption} numberOfLines={1}>
                {t("providerControl.sideQuestions.replyTo", {
                  question:
                    state.records.find((record) => record.id === replyTo)?.question ?? replyTo,
                })}
              </Text>
              <Button variant="ghost" size="sm" onPress={() => setReplyTo(null)}>
                <Text style={styles.caption}>{t("providerControl.sideQuestions.newQuestion")}</Text>
              </Button>
            </View>
          ) : null}
          <View style={styles.compose}>
            <TextInput
              accessibilityLabel={t("providerControl.sideQuestions.question")}
              placeholder={t(
                replyTo
                  ? "providerControl.sideQuestions.followUpPlaceholder"
                  : "providerControl.sideQuestions.newPlaceholder",
              )}
              value={draft}
              onChangeText={setDraft}
              editable={!disabled}
              multiline
              style={styles.input}
              testID="side-question-input"
            />
            <Button
              variant="outline"
              size="sm"
              disabled={disabled || !client || sending || Boolean(state.runningId) || !draft.trim()}
              onPress={() => void send()}
              testID="side-question-send"
            >
              <Text style={styles.caption}>
                {sending ? t("common.loading") : t("providerControl.sideQuestions.ask")}
              </Text>
            </Button>
          </View>
          {error || state.error ? (
            <Text accessibilityRole="alert" style={styles.error}>
              {error ?? state.error}
            </Text>
          ) : null}
        </>
      ) : null}
    </View>
  );
}

const SideTopic = memo(
  function SideTopic({
    record,
    running,
    disabled,
    run,
    onFollowUp,
  }: {
    record: Topic;
    running: boolean;
    disabled: boolean;
    run: (action: string, args: Record<string, string>) => Promise<unknown>;
    onFollowUp: (id: string) => void;
  }) {
    const { t } = useTranslation();
    const [pending, setPending] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const latest = record.turns[record.turns.length - 1]!;
    const phase = latest.status === "running" && !running ? "interrupted" : latest.status;
    const cancel = async () => {
      setPending(true);
      setError(null);
      try {
        await run("btw_cancel", { recordId: record.id });
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : String(cause));
      } finally {
        setPending(false);
      }
    };
    return (
      <View style={styles.topic} testID={`side-question-topic-${record.id}`}>
        <View style={styles.row}>
          <Text style={styles.title} numberOfLines={2}>
            {record.question}
          </Text>
          <StatusBadge
            variant={phase === "error" ? "error" : phase === "complete" ? "success" : "muted"}
            label={t(`providerControl.sideQuestions.states.${phase}`)}
          />
        </View>
        <View style={styles.row}>
          {running ? (
            <Button
              variant="outline"
              size="sm"
              disabled={disabled || pending}
              onPress={() => void cancel()}
            >
              <Text style={styles.caption}>{t("providerControl.sideQuestions.cancel")}</Text>
            </Button>
          ) : (
            <Button
              variant="ghost"
              size="sm"
              disabled={disabled}
              onPress={() => onFollowUp(record.id)}
            >
              <Text style={styles.caption}>{t("providerControl.sideQuestions.followUp")}</Text>
            </Button>
          )}
        </View>
        <ScrollView
          style={styles.history}
          contentContainerStyle={styles.turns}
          nestedScrollEnabled
          testID={`side-question-answer-scroll-${record.id}`}
        >
          {record.turns.map((turn, index) => (
            <View key={`${turn.createdAt}:${index}`} style={styles.turn}>
              {index > 0 || turn.question !== record.question ? (
                <Text style={styles.question} selectable>
                  {turn.question}
                </Text>
              ) : null}
              {turn.answer ? (
                <View testID={`side-question-answer-${record.id}-${index}`}>
                  <MarkdownRenderer
                    text={turn.answer}
                    compact
                    streaming={running && turn.status === "running"}
                  />
                </View>
              ) : null}
              {turn.error ? <Text style={styles.error}>{turn.error}</Text> : null}
            </View>
          ))}
        </ScrollView>
        {record.truncated ? (
          <Text style={styles.caption}>{t("providerControl.sideQuestions.shortened")}</Text>
        ) : null}

        {error ? (
          <Text accessibilityRole="alert" style={styles.error}>
            {error}
          </Text>
        ) : null}
      </View>
    );
  },
  (before, after) =>
    before.running === after.running &&
    before.disabled === after.disabled &&
    before.run === after.run &&
    before.onFollowUp === after.onFollowUp &&
    before.record.truncated === after.record.truncated &&
    before.record.question === after.record.question &&
    before.record.turns.length === after.record.turns.length &&
    before.record.turns.every((turn, index) => {
      const next = after.record.turns[index]!;
      return (
        turn.question === next.question &&
        turn.answer === next.answer &&
        turn.status === next.status &&
        turn.error === next.error &&
        turn.createdAt === next.createdAt &&
        turn.updatedAt === next.updatedAt
      );
    }),
);

const styles = StyleSheet.create((theme) => ({
  panel: { gap: 8 },
  header: { flexDirection: "row", alignItems: "center", gap: 8 },
  headerToggle: { flexShrink: 1 },
  history: { maxHeight: 144 },
  turns: { gap: 8 },
  topicPicker: { maxHeight: 32 },
  topicLabel: { color: theme.colors.foregroundMuted, fontSize: 12, maxWidth: 160 },
  compose: { flexDirection: "row", alignItems: "flex-end", gap: 8 },
  list: { gap: 12 },
  row: { flexDirection: "row", alignItems: "center", gap: 8, flexWrap: "wrap" },
  topic: {
    gap: 8,
    padding: 10,
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: 10,
  },
  turn: { gap: 6 },
  title: { color: theme.colors.foreground, fontSize: 13, fontWeight: "600", flexShrink: 1 },
  question: { color: theme.colors.foregroundMuted, fontSize: 12 },
  answer: { color: theme.colors.foreground, fontSize: 13, lineHeight: 19 },
  caption: { color: theme.colors.foregroundMuted, fontSize: 12 },
  input: {
    color: theme.colors.foreground,
    fontSize: 13,
    padding: 10,
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: 8,
    flex: 1,
    minHeight: 36,
    maxHeight: 72,
  },
  error: { color: theme.colors.statusDanger, fontSize: 12 },
}));

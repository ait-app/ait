import { memo, useCallback, useEffect, useRef, useState } from "react";
import { ScrollView, Text, TextInput, View } from "react-native";
import { Plus, X } from "lucide-react-native";
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
  expanded,
  onEnd,
  branchAvailable = false,
  branchDisabled = false,
  onInputFocus,
}: {
  state: State;
  serverId: string;
  agentId: string;
  disabled: boolean;
  expanded: boolean;
  onEnd: () => void;
  branchAvailable?: boolean;
  branchDisabled?: boolean;
  onInputFocus?: () => void;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const [draft, setDraft] = useState("");
  const [composeOpen, setComposeOpen] = useState(state.records.length === 0);
  const inputRef = useRef<TextInput>(null);
  const [topicId, setTopicId] = useState<string | null>(state.runningId);
  useEffect(() => {
    if (state.runningId) setTopicId(state.runningId);
  }, [state.runningId]);
  const selectedTopic = state.records.find((record) => record.id === topicId) ?? state.records[0];
  const [replyTo, setReplyTo] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [ending, setEnding] = useState(false);
  const [branching, setBranching] = useState(false);
  const run = useCallback(
    async (action: string, arguments_: Record<string, string>) => {
      if (!client) throw new Error(t("providerControl.sideQuestions.disconnected"));
      return client.runNativeAction(agentId, action, arguments_);
    },
    [client, agentId, t],
  );
  const send = async () => {
    if (disabled || ending || branching || sending || state.runningId || !draft.trim()) return;
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
  const followUp = useCallback((id: string) => {
    setReplyTo(id);
    setComposeOpen(true);
  }, []);
  useEffect(() => {
    if (composeOpen && expanded) inputRef.current?.focus();
  }, [composeOpen, expanded, replyTo]);
  const end = async () => {
    if (ending || branching || sending || (state.runningId && (disabled || !client))) return;
    if (!state.runningId) {
      onEnd();
      return;
    }
    setEnding(true);
    setError(null);
    try {
      await run("btw_cancel", { recordId: state.runningId });
      onEnd();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setEnding(false);
    }
  };
  return (
    <View style={styles.panel} testID="provider-side-questions">
      <View style={styles.header}>
        <Text style={[styles.title, styles.headerToggle]}>
          {t("providerControl.sideQuestions.title")}
        </Text>
        <Button
          variant="ghost"
          size="xs"
          disabled={ending || branching || sending}
          leftIcon={Plus}
          onPress={() => {
            setReplyTo(null);
            setComposeOpen(true);
          }}
          testID="side-question-new"
        >
          <Text style={styles.caption}>{t("providerControl.sideQuestions.newQuestion")}</Text>
        </Button>
        <Button
          variant="ghost"
          size="sm"
          leftIcon={X}
          disabled={
            ending || branching || sending || Boolean(state.runningId && (disabled || !client))
          }
          loading={ending}
          accessibilityLabel={t("providerControl.sideQuestions.end")}
          onPress={() => void end()}
          testID="side-question-end"
        >
          {t("providerControl.sideQuestions.end")}
        </Button>
      </View>
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
            {!state.records.length && !composeOpen ? (
              <Text style={styles.caption}>{t("providerControl.sideQuestions.empty")}</Text>
            ) : null}
            {(selectedTopic ? [selectedTopic] : []).map((record) => (
              <SideTopic
                key={record.id}
                record={record}
                running={state.runningId === record.id}
                disabled={disabled || ending || branching || sending}
                anotherRunning={Boolean(state.runningId && state.runningId !== record.id)}
                branchAvailable={branchAvailable}
                branchDisabled={branchDisabled}
                branching={branching}
                onBranch={async (id) => {
                  setBranching(true);
                  setError(null);
                  try {
                    await run("btw_branch", { recordId: id });
                    onEnd();
                  } catch (cause) {
                    setError(cause instanceof Error ? cause.message : String(cause));
                  } finally {
                    setBranching(false);
                  }
                }}
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
          {composeOpen && replyTo ? (
            <View style={styles.row}>
              <Text style={styles.caption} numberOfLines={1}>
                {t("providerControl.sideQuestions.replyTo", {
                  question:
                    state.records.find((record) => record.id === replyTo)?.question ??
                    t("providerControl.sideQuestions.title"),
                })}
              </Text>
            </View>
          ) : null}
          {composeOpen ? (
            <View style={styles.compose}>
              <TextInput
                ref={inputRef}
                accessibilityLabel={t("providerControl.sideQuestions.question")}
                placeholder={t(
                  replyTo
                    ? "providerControl.sideQuestions.followUpPlaceholder"
                    : "providerControl.sideQuestions.newPlaceholder",
                )}
                value={draft}
                onChangeText={setDraft}
                onFocus={() => requestAnimationFrame(() => onInputFocus?.())}
                editable={!disabled}
                multiline
                style={styles.input}
                testID="side-question-input"
              />
              <Button
                variant="outline"
                size="sm"
                disabled={
                  disabled ||
                  !client ||
                  ending ||
                  branching ||
                  sending ||
                  Boolean(state.runningId) ||
                  !draft.trim()
                }
                onPress={() => void send()}
                testID="side-question-send"
              >
                <Text style={styles.caption}>
                  {sending ? t("common.loading") : t("providerControl.sideQuestions.ask")}
                </Text>
              </Button>
            </View>
          ) : null}
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

const SideTopic = memo(function SideTopic({
  record,
  running,
  disabled,
  run,
  onFollowUp,
  onBranch,
  branchAvailable,
  branchDisabled,
  branching,
  anotherRunning,
}: {
  record: Topic;
  running: boolean;
  disabled: boolean;
  run: (action: string, args: Record<string, string>) => Promise<unknown>;
  onFollowUp: (id: string) => void;
  onBranch: (id: string) => Promise<void>;
  branchAvailable: boolean;
  branchDisabled: boolean;
  branching: boolean;
  anotherRunning: boolean;
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
        {running ? (
          <Button
            variant="outline"
            size="sm"
            disabled={disabled || pending}
            onPress={() => void cancel()}
          >
            <Text style={styles.caption}>{t("providerControl.sideQuestions.cancel")}</Text>
          </Button>
        ) : null}
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
      {!running ? (
        <View style={[styles.row, styles.topicActions]}>
          <Button
            variant="ghost"
            size="sm"
            disabled={disabled || anotherRunning}
            onPress={() => onFollowUp(record.id)}
            testID="side-question-follow-up"
          >
            {t("providerControl.sideQuestions.followUp")}
          </Button>
          {branchAvailable &&
          record.turns.length === 1 &&
          !record.truncated &&
          latest.status === "complete" &&
          latest.answer.trim() ? (
            <Button
              variant="ghost"
              size="sm"
              disabled={disabled || anotherRunning || branchDisabled}
              loading={branching}
              onPress={() => void onBranch(record.id)}
              testID="side-question-branch"
            >
              {t("providerControl.sideQuestions.branch")}
            </Button>
          ) : null}
        </View>
      ) : null}
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
});

const styles = StyleSheet.create((theme) => ({
  panel: { gap: 8 },
  header: { flexDirection: "row", alignItems: "center", gap: 8 },
  headerToggle: { flex: 1 },
  history: { maxHeight: 144 },
  turns: { gap: 8 },
  topicPicker: { maxHeight: 32 },
  topicLabel: { color: theme.colors.foregroundMuted, fontSize: 12, maxWidth: 160 },
  compose: { flexDirection: "row", alignItems: "flex-end", gap: 8 },
  list: { gap: 12 },
  row: { flexDirection: "row", alignItems: "center", gap: 8, flexWrap: "wrap" },
  topicActions: { justifyContent: "flex-end" },
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

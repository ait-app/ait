import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import type { SideTopic } from "@ait/protocol/messages";
import { isWeb } from "@/constants/platform";
import { btwTopicTurns } from "@/btw";
import { LoadingSpinner } from "@/components/ui/loading-spinner";
import { MarkdownRenderer } from "@/components/markdown/renderer";

/** Shared transcript for the floating window and its expanded conversation. */
export function BtwConversation({ topic }: { topic: SideTopic }) {
  const { t } = useTranslation();
  const { theme } = useUnistyles();
  return (
    <View style={styles.messages} testID="btw-conversation">
      {btwTopicTurns(topic).map((turn, index) => (
        <View key={`${topic.id}:${index}`} style={styles.turn}>
          <View style={styles.userMessage} testID="side-question-user-bubble">
            <Text selectable style={styles.question} testID="side-question-user-text">
              {turn.question}
            </Text>
          </View>
          {turn.answer ? (
            <View style={styles.answer}>
              <MarkdownRenderer text={turn.answer} streaming={turn.status === "running"} />
            </View>
          ) : null}
          {turn.status === "running" ? (
            <View style={styles.status} accessibilityLiveRegion="polite">
              <LoadingSpinner size="small" color={theme.colors.foregroundMuted} />
              <Text style={styles.muted}>{t("btw.thinking")}</Text>
            </View>
          ) : null}
          {turn.error ? (
            <Text style={styles.error} accessibilityRole="alert">
              {turn.error}
            </Text>
          ) : null}
          {turn.status === "cancelled" || turn.status === "interrupted" ? (
            <Text style={styles.muted}>{t(`btw.states.${turn.status}`)}</Text>
          ) : null}
        </View>
      ))}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  messages: { gap: theme.spacing[6] },
  turn: { gap: theme.spacing[4] },
  userMessage: {
    alignSelf: "flex-end",
    maxWidth: "92%",
    backgroundColor: theme.colors.surface3,
    borderRadius: theme.borderRadius["2xl"],
    borderTopRightRadius: theme.borderRadius.sm,
    paddingHorizontal: theme.spacing[3],
    paddingVertical: theme.spacing[2],
  },
  question: {
    color: theme.colors.foreground,
    fontFamily: theme.fontFamily.ui,
    fontSize: theme.fontSize.content,
    ...(isWeb ? { lineHeight: Math.round(theme.fontSize.content * 1.4) } : {}),
  },
  answer: { paddingHorizontal: theme.spacing[0.5] },
  status: { flexDirection: "row", alignItems: "center", gap: theme.spacing[2] },
  muted: {
    color: theme.colors.foregroundMuted,
    fontFamily: theme.fontFamily.ui,
    fontSize: theme.fontSize.sm,
    lineHeight: Math.round(theme.fontSize.sm * 1.4),
  },
  error: {
    color: theme.colors.destructive,
    fontFamily: theme.fontFamily.ui,
    fontSize: theme.fontSize.sm,
    lineHeight: Math.round(theme.fontSize.sm * 1.4),
  },
}));

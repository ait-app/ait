import { memo } from "react";
import { Text, View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import type { VoiceTranscriptEntry } from "@ait/protocol/agent-types";

/** A speech excerpt belongs to one native handoff, not additional editable text turns. */
export const VoiceTranscript = memo(function VoiceTranscript({
  entries,
  disableOuterSpacing = false,
}: {
  entries: readonly VoiceTranscriptEntry[];
  disableOuterSpacing?: boolean;
}) {
  return (
    <View
      testID="voice-transcript"
      style={[styles.container, !disableOuterSpacing && styles.spacing]}
    >
      {entries.map((entry, index) => (
        <View
          key={index}
          testID={`voice-transcript-${entry.role}`}
          style={entry.role === "user" ? styles.user : styles.assistant}
        >
          <Text selectable style={styles.text}>
            {entry.text}
          </Text>
        </View>
      ))}
    </View>
  );
});

const styles = StyleSheet.create((theme) => ({
  container: {
    width: "100%",
    borderLeftWidth: 3,
    borderLeftColor: theme.colors.border,
    paddingLeft: theme.spacing[4],
    gap: theme.spacing[4],
  },
  spacing: { marginVertical: theme.spacing[4] },
  user: {
    alignSelf: "flex-end",
    maxWidth: "90%",
    backgroundColor: theme.colors.surface3,
    borderRadius: theme.borderRadius["2xl"],
    borderTopRightRadius: theme.borderRadius.sm,
    paddingHorizontal: theme.spacing[4],
    paddingVertical: theme.spacing[3],
  },
  assistant: {
    alignSelf: "flex-start",
    maxWidth: "100%",
    paddingVertical: theme.spacing[2],
  },
  text: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.content,
    lineHeight: Math.round(theme.fontSize.content * 1.4),
  },
}));

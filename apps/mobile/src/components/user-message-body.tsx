import { ChevronRight } from "lucide-react-native";
import { memo, useCallback, useMemo, useState } from "react";
import { Pressable, Text, View, type StyleProp, type TextStyle } from "react-native";
import { StyleSheet, useUnistyles } from "react-native-unistyles";

import { isWeb } from "@/constants/platform";
import { ICON_SIZE } from "@/styles/theme";
import { splitUserMessageTags } from "./user-message-tags";

interface UserMessageBodyProps {
  message: string;
  textStyle: StyleProp<TextStyle>;
  /** Marks prose nodes, e.g. for in-chat find; collapsed blocks are not searchable text. */
  textDataSet?: Record<string, string>;
}

/** Renders user-message text with harness-injected tag blocks collapsed. */
export const UserMessageBody = memo(function UserMessageBody({
  message,
  textStyle,
  textDataSet,
}: UserMessageBodyProps) {
  const segments = useMemo(() => splitUserMessageTags(message), [message]);
  if (segments.length === 1 && segments[0].kind === "text") {
    return (
      <Text selectable style={textStyle} dataSet={textDataSet}>
        {segments[0].text}
      </Text>
    );
  }
  return (
    <View style={styles.segments}>
      {segments.map((segment, index) =>
        segment.kind === "tag" ? (
          <TagBlock key={index} name={segment.name} body={segment.body} />
        ) : (
          <Text key={index} selectable style={textStyle} dataSet={textDataSet}>
            {segment.text}
          </Text>
        ),
      )}
    </View>
  );
});

interface TagBlockProps {
  name: string;
  body: string;
}

/** A harness-injected `<name>…</name>` block, collapsed to its tag name. */
const TagBlock = memo(function TagBlock({ name, body }: TagBlockProps) {
  const { theme } = useUnistyles();
  const [expanded, setExpanded] = useState(false);
  const toggle = useCallback(() => setExpanded((value) => !value), []);
  const hasBody = body.length > 0;

  return (
    <View style={styles.container} testID="user-message-tag-block">
      <Pressable
        onPress={hasBody ? toggle : undefined}
        disabled={!hasBody}
        style={styles.header}
        accessibilityRole="button"
        accessibilityLabel={name}
        accessibilityState={{ expanded, disabled: !hasBody }}
        testID="user-message-tag-toggle"
      >
        <View style={expanded ? styles.chevronExpanded : undefined}>
          <ChevronRight size={ICON_SIZE.xs} color={theme.colors.foregroundMuted} />
        </View>
        <Text style={styles.name} numberOfLines={1}>
          {name}
        </Text>
      </Pressable>
      {expanded ? (
        <Text selectable style={styles.body} testID="user-message-tag-body">
          {body}
        </Text>
      ) : null}
    </View>
  );
});

const styles = StyleSheet.create((theme) => ({
  segments: {
    gap: theme.spacing[2],
  },
  container: {
    alignSelf: "flex-start",
    maxWidth: "100%",
    borderRadius: theme.borderRadius.md,
    borderWidth: 1,
    borderColor: theme.colors.border,
    backgroundColor: theme.colors.surface2,
  },
  header: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[1],
    paddingHorizontal: theme.spacing[2],
    paddingVertical: theme.spacing[1],
  },
  chevronExpanded: {
    transform: [{ rotate: "90deg" }],
  },
  name: {
    flexShrink: 1,
    color: theme.colors.foregroundMuted,
    fontFamily: theme.fontFamily.mono,
    fontSize: theme.fontSize.code,
  },
  body: {
    color: theme.colors.foregroundMuted,
    fontFamily: theme.fontFamily.mono,
    fontSize: theme.fontSize.code,
    paddingHorizontal: theme.spacing[2],
    paddingBottom: theme.spacing[2],
    ...(isWeb ? { whiteSpace: "pre-wrap" as const, overflowWrap: "anywhere" as const } : {}),
  },
}));

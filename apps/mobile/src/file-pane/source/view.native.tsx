import { useEffect, useMemo, useRef } from "react";
import { FlatList, Text, View, type ListRenderItem } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { highlightCode, type HighlightToken } from "@ait/highlight";
import { syntaxTokenStyleFor } from "@/styles/syntax-token-styles";
import type { WorkspaceFileLocation } from "@/workspace/file-open";
import type { EditorVisualTheme } from "../editor/extensions.web";
import { selectSourcePresentation } from "./presentation";

interface FileSourceViewProps {
  content: string;
  filename: string;
  location: WorkspaceFileLocation;
  navigationRevision: number;
  size: number;
  theme: EditorVisualTheme;
  tooLargeMessage: string;
}

// getItemLayout and the row styles must agree, or line navigation lands on the
// wrong line and the virtualized window drifts from what is on screen.
const SOURCE_LINE_HEIGHT = 20;

interface SourceLine {
  number: number;
  tokens: HighlightToken[];
}

export function FileSourceView({
  content,
  filename,
  location,
  navigationRevision,
  size,
  tooLargeMessage,
}: FileSourceViewProps) {
  const presentation = selectSourcePresentation({ size, platform: "native" });
  if (presentation === "unsupported") {
    return (
      <View style={styles.unsupported} testID="file-source-too-large">
        <Text style={styles.unsupportedText}>{tooLargeMessage}</Text>
      </View>
    );
  }
  return (
    <VirtualizedSource
      content={content}
      filename={filename}
      location={location}
      navigationRevision={navigationRevision}
      presentation={presentation}
    />
  );
}

function VirtualizedSource({
  content,
  filename,
  location,
  navigationRevision,
  presentation,
}: Omit<FileSourceViewProps, "size" | "theme" | "tooLargeMessage"> & {
  presentation: "highlighted" | "plain";
}) {
  const listRef = useRef<FlatList<SourceLine>>(null);
  const lines = useMemo(() => {
    // Split on every line break the web editor recognizes; a CR left inside a
    // native Text renders as an extra line break and breaks the fixed row height.
    const source = content.includes("\r") ? content.replace(/\r\n?/g, "\n") : content;
    if (presentation === "highlighted")
      return highlightCode(source, filename).map((tokens, index) => ({
        number: index + 1,
        tokens,
      }));
    return source
      .split("\n")
      .map((text, index) => ({ number: index + 1, tokens: [{ text, style: null }] }));
  }, [content, filename, presentation]);
  useEffect(() => {
    if (!location.lineStart) return;
    listRef.current?.scrollToIndex({
      index: Math.min(location.lineStart - 1, lines.length - 1),
      animated: false,
      viewPosition: 0.5,
    });
  }, [lines.length, location.lineStart, navigationRevision]);
  return (
    <FlatList
      ref={listRef}
      data={lines}
      keyExtractor={sourceLineKey}
      initialNumToRender={24}
      windowSize={9}
      getItemLayout={sourceLineLayout}
      renderItem={renderSourceLine}
    />
  );
}

function SourceLineView({ line }: { line: SourceLine }) {
  return (
    <View style={styles.line}>
      <Text style={styles.gutter}>{line.number}</Text>
      <Text selectable style={styles.text}>
        {line.tokens.map((token, index) => (
          <Text key={index} style={syntaxTokenStyleFor(token.style)}>
            {token.text}
          </Text>
        ))}
      </Text>
    </View>
  );
}

function sourceLineKey(line: SourceLine): string {
  return String(line.number);
}

function sourceLineLayout(_data: ArrayLike<SourceLine> | null | undefined, index: number) {
  return { length: SOURCE_LINE_HEIGHT, offset: index * SOURCE_LINE_HEIGHT, index };
}

const renderSourceLine: ListRenderItem<SourceLine> = ({ item }) => <SourceLineView line={item} />;

const styles = StyleSheet.create((theme) => ({
  unsupported: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
    padding: theme.spacing[4],
  },
  unsupportedText: { color: theme.colors.foregroundMuted, textAlign: "center" },
  line: { flexDirection: "row", minHeight: SOURCE_LINE_HEIGHT },
  gutter: {
    width: 56,
    paddingRight: theme.spacing[3],
    textAlign: "right",
    color: theme.colors.foregroundMuted,
    fontFamily: theme.fontFamily.mono,
    fontSize: theme.fontSize.code,
  },
  text: {
    flex: 1,
    fontFamily: theme.fontFamily.mono,
    fontSize: theme.fontSize.code,
    lineHeight: SOURCE_LINE_HEIGHT,
  },
}));

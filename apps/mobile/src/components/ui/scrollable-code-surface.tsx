import { useMemo, type ReactNode } from "react";
import { Text, View, type StyleProp, type ViewStyle } from "react-native";
import { StyleSheet } from "react-native-unistyles";

import { ScrollView } from "./scroll-view";
import { isWeb } from "@/constants/platform";
import { CODE_SURFACE_DATASET } from "@/styles/code-surface";
import { inlineUnistylesStyle } from "@/styles/unistyles-inline-style";

interface ScrollableCodeSurfaceProps {
  children: ReactNode;
  maxHeight?: number;
  style?: StyleProp<ViewStyle>;
  scrollStyle?: StyleProp<ViewStyle>;
  testID?: string;
}

interface SurfaceCardProps {
  children: ReactNode;
  style?: StyleProp<ViewStyle>;
  testID?: string;
  codeSurface?: boolean;
}

export function SurfaceCard({ children, style, testID, codeSurface = false }: SurfaceCardProps) {
  const containerStyle = useMemo(() => [styles.container, style], [style]);

  return (
    <View
      style={containerStyle}
      dataSet={codeSurface ? CODE_SURFACE_DATASET : undefined}
      testID={testID}
      accessible={false}
    >
      {children}
    </View>
  );
}

export function ScrollableCodeSurface({
  children,
  maxHeight,
  style,
  scrollStyle,
  testID,
}: ScrollableCodeSurfaceProps) {
  const maxHeightStyle = useMemo(
    () => (maxHeight == null ? null : inlineUnistylesStyle({ maxHeight })),
    [maxHeight],
  );
  const outerScrollStyle = useMemo(
    () => [styles.scroll, maxHeightStyle, scrollStyle],
    [maxHeightStyle, scrollStyle],
  );
  const codeContent =
    typeof children === "string" ? (
      <PlainCodeText text={children} />
    ) : (
      <Text selectable style={styles.text} dataSet={CODE_SURFACE_DATASET}>
        {children}
      </Text>
    );

  return (
    <SurfaceCard style={[styles.scroll, style]} testID={testID} codeSurface>
      <ScrollView style={outerScrollStyle} nestedScrollEnabled showsVerticalScrollIndicator>
        <View style={styles.content}>
          <ScrollView horizontal nestedScrollEnabled showsHorizontalScrollIndicator>
            {codeContent}
          </ScrollView>
        </View>
      </ScrollView>
    </SurfaceCard>
  );
}

function PlainCodeText({ text }: { text: string }) {
  const lines = useMemo(() => {
    let offset = 0;
    return text.split("\n").map((line) => {
      const key = `${offset}:${line.length}`;
      offset += line.length + 1;
      return { key, line };
    });
  }, [text]);
  return (
    <View dataSet={CODE_SURFACE_DATASET}>
      {lines.map(({ key, line }) => (
        <Text key={key} selectable style={styles.text} dataSet={CODE_SURFACE_DATASET}>
          {line.length === 0 ? "\u200B" : line}
        </Text>
      ))}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  scroll: {
    flexShrink: 1,
    minHeight: 0,
  },
  container: {
    overflow: "hidden",
    backgroundColor: theme.colors.surface1,
    borderRadius: theme.borderRadius.lg,
    borderWidth: theme.borderWidth[1],
    borderColor: theme.colors.border,
  },
  content: {
    paddingVertical: theme.spacing[3],
    paddingHorizontal: theme.spacing[4],
  },
  text: {
    backgroundColor: "transparent",
    fontFamily: theme.fontFamily.mono,
    fontSize: theme.fontSize.code,
    color: theme.colors.foreground,
    lineHeight: 18,
    ...(isWeb
      ? {
          whiteSpace: "pre",
          overflowWrap: "normal",
        }
      : null),
  },
}));

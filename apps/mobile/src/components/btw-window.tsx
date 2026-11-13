import { useEffect, useRef, useState } from "react";
import {
  Keyboard,
  Platform,
  ScrollView,
  Text,
  TextInput,
  View,
  useWindowDimensions,
} from "react-native";
import { ArrowUp, GitBranch, History, Maximize2, Plus, Square, X } from "lucide-react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import type { SideQuestionsState, SideTopic } from "@ait/protocol/messages";
import { isWeb } from "@/constants/platform";
import { Button } from "@/components/ui/button";
import { createControlGeometry } from "@/components/ui/control-geometry";
import { ICON_SIZE, ICON_STROKE_WIDTH, SPACING } from "@/styles/theme";
import { LoadingSpinner } from "@/components/ui/loading-spinner";
import { BtwConversation } from "@/components/btw-conversation";
import { isImeComposingKeyboardEvent } from "@/utils/keyboard-ime";

export interface BtwWindowProps {
  sourceTitle: string;
  state: SideQuestionsState;
  topic: SideTopic | null;
  draft: string;
  error: string | null;
  loading: boolean;
  pending: boolean;
  disabled: boolean;
  historyOpen: boolean;
  canTransfer: boolean;
  closeBlocked?: boolean;
  onDraftChange: (text: string) => void;
  onSubmit: () => void;
  onCancel: () => void;
  onClose: () => void;
  onTransfer: () => void;
  onNew: () => void;
  onHistoryToggle: () => void;
  onSelectTopic: (id: string) => void;
  onRetry: () => void;
}

/** The same small conversation surface on desktop, web and mobile. */
export function BtwWindow({
  sourceTitle,
  state,
  topic,
  draft,
  error,
  loading,
  pending,
  disabled,
  historyOpen,
  canTransfer,
  closeBlocked,
  onDraftChange,
  onSubmit,
  onCancel,
  onClose,
  onTransfer,
  onNew,
  onHistoryToggle,
  onSelectTopic,
  onRetry,
}: BtwWindowProps) {
  const { t } = useTranslation();
  const { theme } = useUnistyles();
  const inputRef = useRef<TextInput>(null);
  const scrollRef = useRef<ScrollView>(null);
  const nearBottom = useRef(true);
  const { height, width } = useWindowDimensions();
  const controlSize = width < 640 ? "md" : "sm";
  const [inputFocused, setInputFocused] = useState(false);
  const [inputHovered, setInputHovered] = useState(false);
  const [keyboardHeight, setKeyboardHeight] = useState(0);
  const running = Boolean(state.runningId);
  const hasTurns = Boolean(topic);

  useEffect(() => {
    const input = inputRef.current;
    // The shared native type omits the browser's `outline-style: none` value.
    // The surrounding composer draws the accessible focus ring instead.
    if (isWeb && typeof HTMLElement !== "undefined" && input instanceof HTMLElement) {
      input.style.outline = "none";
    }
    input?.focus();
  }, [topic?.id]);

  useEffect(() => {
    if (isWeb || Platform.OS !== "ios") return;
    const shown = Keyboard.addListener("keyboardWillChangeFrame", (event) =>
      setKeyboardHeight(event.endCoordinates.height),
    );
    const hidden = Keyboard.addListener("keyboardWillHide", () => setKeyboardHeight(0));
    return () => {
      shown.remove();
      hidden.remove();
    };
  }, []);

  useEffect(() => {
    nearBottom.current = true;
    scrollRef.current?.scrollToEnd({ animated: false });
  }, [topic?.id]);

  return (
    <View
      style={[
        styles.window,
        { height: Math.max(220, Math.min(560, height - keyboardHeight - SPACING[24])) },
      ]}
      testID="btw-window"
      accessibilityLabel={t("btw.title")}
    >
      <View style={styles.header}>
        <View style={styles.heading}>
          <Text style={styles.title}>{t("btw.title")}</Text>
        </View>
        <Button
          variant="ghost"
          size={controlSize}
          leftIcon={Plus}
          accessibilityLabel={t("btw.newConversation")}
          onPress={onNew}
          disabled={pending || running}
          testID="side-question-new"
        />
        <Button
          variant="ghost"
          size={controlSize}
          leftIcon={History}
          accessibilityLabel={t("btw.history")}
          accessibilityState={{ expanded: historyOpen }}
          onPress={onHistoryToggle}
          testID="side-question-history"
        />
        <Button
          variant="ghost"
          size={controlSize}
          leftIcon={Maximize2}
          accessibilityLabel={t("btw.expand")}
          onPress={onTransfer}
          disabled={!canTransfer || pending || running}
          testID="side-question-expand"
        />
        <Button
          variant="ghost"
          size={controlSize}
          leftIcon={X}
          accessibilityLabel={t("btw.close")}
          disabled={pending || closeBlocked || (running && disabled)}
          onPress={onClose}
          testID="side-question-end"
        />
      </View>
      <View style={styles.source} testID="side-question-source">
        <GitBranch
          size={ICON_SIZE.xs}
          strokeWidth={ICON_STROKE_WIDTH}
          color={theme.colors.foregroundMuted}
        />
        <Text style={styles.sourceText} numberOfLines={1}>
          {t("btw.source", { title: sourceTitle })}
        </Text>
      </View>
      {historyOpen ? (
        <ScrollView style={styles.history} contentContainerStyle={styles.historyList}>
          {state.records.length ? (
            state.records.map((record) => (
              <Button
                key={record.id}
                variant={topic?.id === record.id ? "secondary" : "ghost"}
                size={controlSize}
                onPress={() => onSelectTopic(record.id)}
                accessibilityState={{ selected: topic?.id === record.id }}
                testID={`side-question-topic-pick-${record.id}`}
                style={styles.historyItem}
              >
                <Text numberOfLines={1} style={styles.historyText}>
                  {record.title ?? record.question}
                </Text>
              </Button>
            ))
          ) : (
            <Text style={styles.muted}>{t("btw.emptyHistory")}</Text>
          )}
        </ScrollView>
      ) : null}
      <ScrollView
        ref={scrollRef}
        style={styles.transcript}
        contentContainerStyle={hasTurns ? styles.messages : styles.empty}
        keyboardShouldPersistTaps="handled"
        onScroll={({ nativeEvent }) => {
          nearBottom.current =
            nativeEvent.contentSize.height -
              nativeEvent.layoutMeasurement.height -
              nativeEvent.contentOffset.y <
            72;
        }}
        scrollEventThrottle={80}
        onContentSizeChange={() => {
          if (nearBottom.current) scrollRef.current?.scrollToEnd({ animated: false });
        }}
        testID="side-question-transcript"
      >
        {loading ? <LoadingSpinner size="small" color={theme.colors.foregroundMuted} /> : null}
        {!loading && !hasTurns ? (
          <>
            <Text style={styles.emptyTitle}>{t("btw.emptyTitle")}</Text>
            <Text style={styles.emptyDescription}>{t("btw.emptyDescription")}</Text>
          </>
        ) : null}
        {topic ? <BtwConversation topic={topic} /> : null}
        {topic?.truncated ? <Text style={styles.muted}>{t("btw.shortened")}</Text> : null}
      </ScrollView>
      {error ? (
        <View style={styles.errorRow}>
          <Text style={[styles.error, styles.errorText]} accessibilityRole="alert">
            {error}
          </Text>
          <Button variant="ghost" size="xs" onPress={onRetry} disabled={pending}>
            {t("common.actions.retry")}
          </Button>
        </View>
      ) : null}
      <View style={styles.footer}>
        <View
          style={[
            styles.compose,
            inputHovered ? styles.composeHovered : null,
            inputFocused ? styles.composeFocused : null,
          ]}
          onPointerEnter={() => setInputHovered(true)}
          onPointerLeave={() => setInputHovered(false)}
          testID="side-question-compose"
        >
          <TextInput
            ref={inputRef}
            value={draft}
            onChangeText={onDraftChange}
            onFocus={() => setInputFocused(true)}
            onBlur={() => setInputFocused(false)}
            placeholder={topic ? t("btw.followUpPlaceholder") : t("btw.newPlaceholder")}
            placeholderTextColor={theme.colors.foregroundMuted}
            accessibilityLabel={t("btw.question")}
            multiline
            editable={!disabled}
            style={styles.input}
            onKeyPress={(event) => {
              const nativeEvent = event.nativeEvent as typeof event.nativeEvent & {
                shiftKey?: boolean;
                isComposing?: boolean;
                keyCode?: number;
              };
              if (
                isWeb &&
                nativeEvent.key === "Enter" &&
                !nativeEvent.shiftKey &&
                !isImeComposingKeyboardEvent(nativeEvent)
              ) {
                event.preventDefault();
                event.stopPropagation();
                onSubmit();
              }
            }}
            testID="side-question-input"
          />
          {running ? (
            <Button
              variant="secondary"
              size={controlSize}
              leftIcon={Square}
              accessibilityLabel={t("btw.stop")}
              onPress={onCancel}
              disabled={disabled || pending}
              testID="side-question-cancel"
            />
          ) : (
            <Button
              variant="default"
              size={controlSize}
              leftIcon={ArrowUp}
              accessibilityLabel={t("btw.send")}
              onPress={onSubmit}
              disabled={!draft.trim() || disabled || pending || loading}
              loading={pending}
              testID="side-question-ask"
            />
          )}
        </View>
        <Text style={styles.footerText}>{t("btw.footer")}</Text>
      </View>
    </View>
  );
}

const styles = StyleSheet.create((theme) => {
  const geometry = createControlGeometry(theme);
  const metadataText = {
    fontFamily: theme.fontFamily.ui,
    fontSize: theme.fontSize.sm,
    lineHeight: Math.round(theme.fontSize.sm * 1.4),
  };
  return {
    window: {
      backgroundColor: theme.colors.surface1,
      borderWidth: theme.borderWidth[1],
      borderColor: theme.colors.borderAccent,
      borderRadius: theme.borderRadius["2xl"],
      overflow: "hidden",
      ...theme.shadow.lg,
    },
    header: {
      padding: theme.spacing[2],
      paddingLeft: theme.spacing[4],
      flexDirection: "row",
      gap: theme.spacing[0.5],
      alignItems: "center",
    },
    heading: { flex: 1, minWidth: 0, gap: theme.spacing[0.5] },
    title: {
      color: theme.colors.foreground,
      fontFamily: theme.fontFamily.ui,
      fontSize: theme.fontSize.base,
      fontWeight: theme.fontWeight.semibold,
    },
    source: {
      flexDirection: "row",
      alignItems: "center",
      gap: theme.spacing[1.5],
      paddingHorizontal: theme.spacing[4],
      paddingBottom: theme.spacing[3],
      borderBottomWidth: theme.borderWidth[1],
      borderColor: theme.colors.borderAccent,
    },
    sourceText: { ...metadataText, color: theme.colors.foregroundMuted, flexShrink: 1 },
    transcript: { flex: 1, minHeight: 0 },
    messages: { padding: theme.spacing[4], gap: theme.spacing[6] },
    muted: { ...metadataText, color: theme.colors.foregroundMuted },
    empty: {
      flexGrow: 1,
      alignItems: "center",
      justifyContent: "center",
      padding: theme.spacing[6],
      gap: theme.spacing[2],
    },
    emptyTitle: {
      color: theme.colors.foreground,
      fontFamily: theme.fontFamily.ui,
      fontSize: theme.fontSize.lg,
      fontWeight: theme.fontWeight.medium,
      textAlign: "center",
    },
    emptyDescription: {
      color: theme.colors.foregroundMuted,
      fontFamily: theme.fontFamily.ui,
      fontSize: theme.fontSize.base,
      lineHeight: Math.round(theme.fontSize.base * 1.4),
      textAlign: "center",
      maxWidth: 260,
    },
    footer: { padding: theme.spacing[3], paddingTop: theme.spacing[2], gap: theme.spacing[2] },
    compose: {
      ...geometry.controlRest,
      flexDirection: "row",
      alignItems: "flex-end",
      gap: theme.spacing[2],
      padding: theme.spacing[1.5],
      borderRadius: theme.borderRadius["2xl"],
      backgroundColor: theme.colors.surface0,
      borderColor: theme.colors.border,
    },
    composeHovered: geometry.controlHover,
    composeFocused: geometry.controlActive,
    input: {
      borderWidth: theme.borderWidth[0],
      backgroundColor: "transparent",
      color: theme.colors.foreground,
      fontFamily: theme.fontFamily.ui,
      fontSize: theme.fontSize.content,
      fontWeight: theme.fontWeight.normal,
      ...(isWeb ? { lineHeight: Math.round(theme.fontSize.content * 1.4) } : {}),
      paddingHorizontal: theme.spacing[1],
      paddingVertical: theme.spacing[1],
      flex: 1,
      minHeight: theme.spacing[8],
      maxHeight: theme.spacing[16] + theme.spacing[12],
      outlineWidth: 0,
    },
    footerText: { ...metadataText, color: theme.colors.foregroundMuted, textAlign: "center" },
    errorRow: {
      paddingHorizontal: theme.spacing[4],
      paddingVertical: theme.spacing[2],
      flexDirection: "row",
      gap: theme.spacing[2],
      alignItems: "center",
    },
    error: { ...metadataText, color: theme.colors.destructive },
    errorText: { flex: 1 },
    history: {
      maxHeight: 190,
      borderBottomWidth: theme.borderWidth[1],
      borderColor: theme.colors.borderAccent,
    },
    historyList: { padding: theme.spacing[2], gap: theme.spacing[0.5] },
    historyItem: { justifyContent: "flex-start" },
    historyText: { ...metadataText, color: theme.colors.foreground },
  };
});

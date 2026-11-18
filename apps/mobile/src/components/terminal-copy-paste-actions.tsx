import { useCallback } from "react";
import { ClipboardPaste } from "lucide-react-native";
import { useTranslation } from "react-i18next";
import { Pressable, Text, type PressableStateCallbackType } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { TerminalKeyCap } from "./terminal-key-cap";

export interface TerminalPasteActionProps {
  hasClipboardText: boolean;
  onPaste: () => void;
}

export function TerminalPasteAction({ hasClipboardText, onPaste }: TerminalPasteActionProps) {
  const { t } = useTranslation();
  return (
    <TerminalKeyCap
      testID="terminal-paste"
      accessibilityLabel={t("workspace.terminal.paste")}
      icon={ClipboardPaste}
      tone="utility"
      disabled={!hasClipboardText}
      onPress={onPaste}
    />
  );
}

export interface TerminalFloatingCopyActionProps {
  hasSelection: boolean;
  onCopy: () => void;
}

export function TerminalFloatingCopyAction({
  hasSelection,
  onCopy,
}: TerminalFloatingCopyActionProps) {
  const { t } = useTranslation();
  if (!hasSelection) {
    return null;
  }

  return <FloatingCopyButton label={t("common.actions.copy")} onPress={onCopy} />;
}

interface FloatingCopyButtonProps {
  label: string;
  onPress: () => void;
}

function FloatingCopyButton({ label, onPress }: FloatingCopyButtonProps) {
  const pressableStyle = useCallback(
    ({ hovered, pressed }: PressableStateCallbackType & { hovered?: boolean }) => [
      styles.floatingButton,
      (Boolean(hovered) || pressed) && styles.floatingButtonHovered,
    ],
    [],
  );

  return (
    <Pressable
      accessibilityLabel={label}
      accessibilityRole="button"
      testID="terminal-copy"
      onPress={onPress}
      style={pressableStyle}
    >
      <Text style={styles.floatingButtonText}>{label}</Text>
    </Pressable>
  );
}

const styles = StyleSheet.create((theme) => ({
  floatingButton: {
    borderRadius: theme.borderRadius.lg,
    paddingHorizontal: theme.spacing[3],
    paddingVertical: theme.spacing[1],
    backgroundColor: "rgba(32, 32, 32, 0.86)",
  },
  floatingButtonHovered: {
    backgroundColor: "rgba(48, 48, 48, 0.92)",
  },
  floatingButtonText: {
    color: "#f5f5f5",
    fontSize: theme.fontSize.sm,
    fontWeight: theme.fontWeight.medium,
    textAlign: "center",
  },
}));

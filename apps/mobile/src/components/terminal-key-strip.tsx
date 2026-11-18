import { useCallback } from "react";
import { View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import {
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  CornerDownLeft,
  Keyboard as KeyboardIcon,
  KeyboardOff as KeyboardOffIcon,
} from "lucide-react-native";
import { useTranslation } from "react-i18next";
import type { TerminalKeyModifierState } from "@/terminal/runtime/terminal-key-dispatch";
import {
  getTerminalVirtualKeyboardSlotId,
  TERMINAL_VIRTUAL_KEYBOARD_CLUSTERS,
  type TerminalVirtualKeyboardClusterId,
  type TerminalVirtualKeyboardSlot,
  type TerminalVirtualKeyButton,
} from "@/terminal/runtime/terminal-virtual-keyboard";
import { TerminalPasteAction } from "./terminal-copy-paste-actions";
import { TerminalKeyCap, TERMINAL_KEY_CAP_HIT_SLOP } from "./terminal-key-cap";

type TerminalModifier = keyof TerminalKeyModifierState;

const ROW_INSET = TERMINAL_KEY_CAP_HIT_SLOP;

const MODIFIER_LABELS = {
  ctrl: "Ctrl",
  shift: "Shift",
  alt: "Alt",
} as const satisfies Record<TerminalModifier, string>;

// Keys whose legend is a glyph. An icon keeps the same weight on every platform font,
// where the text arrows fell back to whatever the system face happened to draw.
const KEY_ICONS: Partial<Record<TerminalVirtualKeyButton["id"], typeof ArrowUp>> = {
  up: ArrowUp,
  down: ArrowDown,
  left: ArrowLeft,
  right: ArrowRight,
  enter: CornerDownLeft,
};

interface KeyButtonProps {
  button: TerminalVirtualKeyButton;
  onSend: (key: string) => void;
}

function KeyButton({ button, onSend }: KeyButtonProps) {
  const handlePress = useCallback(() => onSend(button.key), [onSend, button.key]);
  return (
    <TerminalKeyCap
      testID={`terminal-key-${button.id}`}
      accessibilityLabel={button.label}
      icon={KEY_ICONS[button.id]}
      label={button.label}
      onPress={handlePress}
    />
  );
}

interface ModifierButtonProps {
  modifier: TerminalModifier;
  active: boolean;
  onToggle: (modifier: TerminalModifier) => void;
}

function ModifierButton({ modifier, active, onToggle }: ModifierButtonProps) {
  const handlePress = useCallback(() => onToggle(modifier), [onToggle, modifier]);
  return (
    <TerminalKeyCap
      testID={`terminal-key-${modifier}`}
      accessibilityLabel={MODIFIER_LABELS[modifier]}
      label={MODIFIER_LABELS[modifier]}
      latched={active}
      onPress={handlePress}
    />
  );
}

interface KeyboardToggleButtonProps {
  isKeyboardVisible: boolean;
  onToggle: () => void;
}

function KeyboardToggleButton({ isKeyboardVisible, onToggle }: KeyboardToggleButtonProps) {
  const { t } = useTranslation();
  return (
    <TerminalKeyCap
      testID="terminal-keyboard-toggle"
      accessibilityLabel={
        isKeyboardVisible
          ? t("workspace.terminal.hideKeyboard")
          : t("workspace.terminal.showKeyboard")
      }
      icon={isKeyboardVisible ? KeyboardOffIcon : KeyboardIcon}
      tone="utility"
      onPress={onToggle}
    />
  );
}

export interface TerminalKeyStripProps {
  modifiers: TerminalKeyModifierState;
  isKeyboardVisible: boolean;
  showPaste: boolean;
  hasClipboardText: boolean;
  onSendKey: (key: string) => void;
  onToggleModifier: (modifier: TerminalModifier) => void;
  onPaste: () => void;
  onToggleKeyboard: () => void;
}

export function TerminalKeyStrip({
  modifiers,
  isKeyboardVisible,
  showPaste,
  hasClipboardText,
  onSendKey,
  onToggleModifier,
  onPaste,
  onToggleKeyboard,
}: TerminalKeyStripProps) {
  const renderSlot = (slot: TerminalVirtualKeyboardSlot) => {
    const slotId = getTerminalVirtualKeyboardSlotId(slot);
    switch (slot.type) {
      case "key":
        return <KeyButton key={slotId} button={slot.button} onSend={onSendKey} />;
      case "modifier":
        return (
          <ModifierButton
            key={slotId}
            modifier={slot.modifier}
            active={modifiers[slot.modifier]}
            onToggle={onToggleModifier}
          />
        );
      case "paste":
        return showPaste ? (
          <TerminalPasteAction key={slotId} hasClipboardText={hasClipboardText} onPaste={onPaste} />
        ) : null;
      case "keyboardToggle":
        return (
          <KeyboardToggleButton
            key={slotId}
            isKeyboardVisible={isKeyboardVisible}
            onToggle={onToggleKeyboard}
          />
        );
      case "gap":
        return <View key={slotId} style={styles.gap} />;
    }
  };

  return (
    <View style={styles.strip} testID="terminal-virtual-keyboard">
      {TERMINAL_VIRTUAL_KEYBOARD_CLUSTERS.map((cluster) => (
        <View key={cluster.id} style={clusterStyle(cluster.id)}>
          {cluster.rows.map((row) => (
            <View key={row.map(getTerminalVirtualKeyboardSlotId).join(":")} style={styles.row}>
              {row.map(renderSlot)}
            </View>
          ))}
        </View>
      ))}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  strip: {
    flexDirection: "row",
    gap: theme.spacing[3],
    borderTopWidth: 1,
    borderTopColor: theme.colors.border,
    backgroundColor: theme.colors.surface0,
    paddingHorizontal: theme.spacing[2],
    paddingVertical: ROW_INSET,
  },
  // Text legends need more room than glyphs, so the clusters share the width unevenly:
  // the named keys get the most, the arrows stay square, and Enter spans two utilities.
  keysCluster: {
    flex: 3.3,
    minWidth: 0,
  },
  arrowsCluster: {
    flex: 3.1,
    minWidth: 0,
  },
  actionsCluster: {
    flex: 1.9,
    minWidth: 0,
  },
  // Rows touch and carry the vertical spacing as padding. Android clips a cap's hit slop
  // to its parent, so the slop has to fall inside the row to count.
  row: {
    flexDirection: "row",
    gap: theme.spacing[1.5],
    paddingVertical: ROW_INSET,
  },
  gap: {
    flex: 1,
    minWidth: 0,
  },
}));

function clusterStyle(id: TerminalVirtualKeyboardClusterId) {
  switch (id) {
    case "keys":
      return styles.keysCluster;
    case "arrows":
      return styles.arrowsCluster;
    case "actions":
      return styles.actionsCluster;
  }
}

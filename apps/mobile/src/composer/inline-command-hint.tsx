import { useSyncExternalStore } from "react";
import { Text, type StyleProp, type TextStyle } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { useAgentCommandsQuery, type DraftCommandConfig } from "@/hooks/use-agent-commands-query";
import type { ComposerTextSource } from "./text-source";
import {
  commandArgumentHint,
  getImeComposingSnapshot,
  isCommandArgumentArea,
  subscribeToImeComposition,
} from "./command-argument-hint";

/** Decorative only: clipboard, history, IME and the submitted draft remain owned by the input. */
export function InlineCommandHint({
  text,
  serverId,
  agentId,
  draftConfig,
  style,
}: {
  text: ComposerTextSource;
  serverId: string;
  agentId: string;
  draftConfig?: DraftCommandConfig;
  style?: StyleProp<TextStyle>;
}) {
  const value = useSyncExternalStore(text.subscribe, text.getSnapshot, text.getSnapshot);
  const composing = useSyncExternalStore(
    subscribeToImeComposition,
    getImeComposingSnapshot,
    getImeComposingSnapshot,
  );
  const isArgumentArea = isCommandArgumentArea(value);
  const { commands } = useAgentCommandsQuery({
    serverId,
    agentId,
    draftConfig,
    enabled: isArgumentArea,
  });
  const hint = !composing && isArgumentArea ? commandArgumentHint(value, commands) : null;
  if (!hint) return null;
  return (
    <Text
      style={[style, styles.hint]}
      pointerEvents="none"
      accessible={false}
      selectable={false}
      testID="composer-command-argument-hint"
    >
      {/* Every separator character keeps the hint aligned past the typed command. */}
      <Text style={styles.prefix}>{hint.prefix.replace(/\s/g, "\u00a0")}</Text>
      {hint.hint}
    </Text>
  );
}
const styles = StyleSheet.create((theme) => ({
  hint: {
    color: theme.colors.foregroundMuted,
    opacity: 0.65,
    position: "absolute",
    top: 0,
    left: 0,
    right: 0,
  },
  prefix: { opacity: 0 },
}));

import { useSyncExternalStore } from "react";
import { Text } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { useAgentCommandsQuery, type DraftCommandConfig } from "@/hooks/use-agent-commands-query";
import type { ComposerTextSource } from "./text-source";
import { commandArgumentHint } from "./command-argument-hint";

export function CommandArgumentHint({
  text,
  serverId,
  agentId,
  draftConfig,
}: {
  text: ComposerTextSource;
  serverId: string;
  agentId: string;
  draftConfig?: DraftCommandConfig;
}) {
  const value = useSyncExternalStore(text.subscribe, text.getSnapshot, text.getSnapshot);
  const active = /^\/[^\s/]+[ \t]+$/u.test(value);
  const { commands } = useAgentCommandsQuery({ serverId, agentId, draftConfig, enabled: active });
  const hint = active ? commandArgumentHint(value, commands) : null;
  return hint ? (
    <Text style={styles.hint} testID="composer-command-argument-hint">
      {hint}
    </Text>
  ) : null;
}

const styles = StyleSheet.create((theme) => ({
  hint: {
    color: theme.colors.foregroundMuted,
    fontSize: 12,
  },
}));

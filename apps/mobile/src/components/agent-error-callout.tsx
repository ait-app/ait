import React from "react";
import { View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import type { AgentLifecycleStatus } from "@ait/protocol/agent-lifecycle";
import { Notification } from "@/components/message";
import type { Theme } from "@/styles/theme";

export function AgentErrorCallout({
  status,
  lastError,
}: {
  status: AgentLifecycleStatus | null;
  lastError?: string | null;
}) {
  const { t } = useTranslation();
  if (status !== "error") return null;

  // Use the persisted state, since lifecycle events are not display timeline items.
  // This also surfaces failures when reopening a chat after the live event was missed.
  return (
    <View style={styles.container} accessibilityRole="alert" testID="agent-execution-error">
      <Notification
        level="error"
        message={lastError?.trim() || t("providerSelection.unknownError")}
        disableOuterSpacing
      />
    </View>
  );
}

const styles = StyleSheet.create((theme: Theme) => ({
  container: {
    width: "100%",
    maxWidth: theme.contentMaxWidth,
    alignSelf: "center",
    padding: theme.spacing[4],
  },
}));

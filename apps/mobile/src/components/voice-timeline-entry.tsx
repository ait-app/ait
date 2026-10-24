import { memo } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import type { VoiceTimelineItem } from "@ait/protocol/agent-types";
import { VoiceTranscript } from "./voice-transcript";

export const VoiceTimelineEntry = memo(function VoiceTimelineEntry({
  item,
}: {
  item: VoiceTimelineItem;
}) {
  const { t } = useTranslation();
  if (item.kind === "transcript") {
    if (!item.text) return null;
    return <VoiceTranscript entries={[{ role: item.role, text: item.text }]} />;
  }
  return (
    <View style={styles.marker} testID="voice-session-marker">
      <Text style={styles.label}>{t(`voiceTimeline.${item.status}`)}</Text>
    </View>
  );
});

const styles = StyleSheet.create((theme) => ({
  marker: { alignItems: "center", paddingVertical: theme.spacing[3] },
  label: { color: theme.colors.foregroundMuted, fontSize: theme.fontSize.sm },
}));

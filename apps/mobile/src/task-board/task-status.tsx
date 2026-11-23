import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import { Circle, CircleCheck, CircleDot, CircleAlert } from "lucide-react-native";
import type { WorkspaceStateBucket } from "@ait/protocol/agent-state-bucket";
import type { Theme } from "@/styles/theme";

export function taskStatusColor(theme: Theme, bucket: WorkspaceStateBucket): string {
  if (bucket === "needs_input") return theme.colors.statusWarning;
  if (bucket === "failed") return theme.colors.statusDanger;
  if (bucket === "running") return theme.colors.statusDotRunning;
  return theme.colors.statusSuccess;
}

export function TaskStatusIcon({
  bucket,
  size = 12,
}: {
  bucket: WorkspaceStateBucket;
  size?: number;
}) {
  const { theme } = useUnistyles();
  const Icon =
    bucket === "needs_input"
      ? Circle
      : bucket === "failed"
        ? CircleAlert
        : bucket === "done"
          ? CircleCheck
          : CircleDot;
  return <Icon size={size} color={taskStatusColor(theme, bucket)} />;
}

export function TaskStatus({ bucket }: { bucket: WorkspaceStateBucket }) {
  const { theme } = useUnistyles();
  const { t } = useTranslation();
  return (
    <View style={styles.row}>
      <TaskStatusIcon bucket={bucket} />
      <Text style={[styles.text, { color: taskStatusColor(theme, bucket) }]}>
        {t(`taskBoard.status.${bucket}`)}
      </Text>
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  row: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[1.5],
    minWidth: 0,
    flexShrink: 1,
  },
  text: { fontSize: theme.fontSize.sm, lineHeight: 18, flexShrink: 1 },
}));

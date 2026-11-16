import { Circle, CircleCheck, CircleDot } from "lucide-react-native";
import { memo } from "react";
import { Text, View } from "react-native";
import { StyleSheet, withUnistyles } from "react-native-unistyles";
import type { Theme } from "@/styles/theme";
import type { TodoEntry } from "@/types/stream";

const ThemedCircle = withUnistyles(Circle);
const ThemedCircleCheck = withUnistyles(CircleCheck);
const ThemedCircleDot = withUnistyles(CircleDot);

const extraMutedIcon = (theme: Theme) => ({ color: theme.colors.foregroundExtraMuted });
const runningIcon = (theme: Theme) => ({ color: theme.colors.statusDotRunning });

function TaskStatusIcon({ isCompleted, isRunning }: { isCompleted: boolean; isRunning: boolean }) {
  if (isCompleted) {
    return <ThemedCircleCheck size={16} uniProps={extraMutedIcon} />;
  }
  if (isRunning) {
    return <ThemedCircleDot size={16} uniProps={runningIcon} />;
  }
  // A pending task's ring is a status mark, not a checkbox. At the muted step it carries the
  // weight of an enabled control and invites a click that does nothing, so it sits one step back
  // from the text it marks.
  return <ThemedCircle size={16} uniProps={extraMutedIcon} />;
}

export const TaskListRow = memo(function TaskListRow({
  task,
  numberOfLines = 1,
  number,
}: {
  task: TodoEntry;
  numberOfLines?: number;
  number?: string;
}) {
  const isCompleted = task.completed || task.status === "completed";
  const isRunning = !isCompleted && task.status === "in_progress";
  const text = isRunning && task.activeForm ? task.activeForm : task.text;

  return (
    <View
      style={[styles.row, numberOfLines !== 1 && styles.multilineRow]}
      accessibilityLabel={number ? `${number} ${text}` : text}
    >
      {number ? <Text style={styles.number}>{number}</Text> : null}
      <TaskStatusIcon isCompleted={isCompleted} isRunning={isRunning} />
      <Text
        numberOfLines={numberOfLines}
        style={[styles.text, isRunning && styles.runningText, isCompleted && styles.completedText]}
      >
        {text}
      </Text>
    </View>
  );
});

const styles = StyleSheet.create((theme) => ({
  row: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
  },
  multilineRow: {
    alignItems: "flex-start",
  },
  number: {
    minWidth: 24,
    textAlign: "right",
    color: theme.colors.foregroundExtraMuted,
    fontFamily: theme.fontFamily.mono,
    fontSize: theme.fontSize.sm,
  },
  // Grows and shrinks, but keeps an `auto` basis: a zero-basis label reports no intrinsic width,
  // and a container that sizes itself to its content — the composer track panel — measures the
  // row as empty and truncates it against a surface that had room to spare.
  text: {
    flexGrow: 1,
    flexShrink: 1,
    flexBasis: "auto",
    minWidth: 0,
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.base,
  },
  runningText: {
    color: theme.colors.foreground,
  },
  completedText: {
    color: theme.colors.foregroundExtraMuted,
    textDecorationLine: "line-through",
  },
}));

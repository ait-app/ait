import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { useTaskBoardModel } from "./use-task-board";
import { taskLane, type TaskLane } from "./model";
import { TaskStatusIcon } from "./task-status";

export function SidebarTaskCounts() {
  const { t } = useTranslation();
  const { roots } = useTaskBoardModel();
  const counts: Record<TaskLane, number> = { needs_action: 0, running: 0, review: 0, done: 0 };
  for (const task of roots) counts[taskLane(task.bucket)] += 1;
  return (
    <View style={styles.row} testID="sidebar-task-counts">
      {(["needs_action", "running", "review"] as const).map((lane) => (
        <View
          style={styles.count}
          key={lane}
          accessibilityLabel={`${t(`taskBoard.lanes.${lane}`)} ${counts[lane]}`}
        >
          <TaskStatusIcon
            bucket={
              lane === "needs_action" ? "needs_input" : lane === "review" ? "attention" : "running"
            }
            size={10}
          />
          <Text style={styles.text}>{counts[lane]}</Text>
        </View>
      ))}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  row: { flexDirection: "row", alignItems: "center", gap: theme.spacing[2] },
  count: { flexDirection: "row", alignItems: "center", gap: theme.spacing[1] },
  text: { fontSize: theme.fontSize.sm, color: theme.colors.foregroundMuted },
}));

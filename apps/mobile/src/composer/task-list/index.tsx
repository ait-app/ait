import { memo, useMemo } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { ComposerTrackPill, ComposerTrackRow } from "@/composer/tracks";
import { TaskListRow } from "@/components/task-list-row";
import type { TodoEntry } from "@/types/stream";
import { presentTaskList } from "./presentation";

export const AgentTaskList = memo(function AgentTaskList({
  tasks,
}: {
  tasks: TodoEntry[] | undefined;
}) {
  if (!tasks?.length) return null;
  return <TaskListCard tasks={tasks} />;
});

const TaskListCard = memo(function TaskListCard({ tasks }: { tasks: TodoEntry[] }) {
  const { t } = useTranslation();
  const completed = useMemo(
    () => tasks.filter((task) => task.completed || task.status === "completed").length,
    [tasks],
  );
  const progress = t("message.todo.tasksProgress", { completed, total: tasks.length });
  const label = `TODO ${completed}/${tasks.length}`;
  const segments = useMemo(() => [{ bucket: null, text: label }], [label]);
  const sections = useMemo(() => presentTaskList(tasks), [tasks]);

  return (
    <ComposerTrackPill
      testID="agent-task-list-header"
      segments={segments}
      panelTitle={t("message.todo.title")}
      accessibilityLabel={`TODO, ${progress}`}
    >
      {sections.map((section) => (
        <View key={section.id}>
          {section.title ? (
            <View style={styles.phaseHeader}>
              {section.number ? <Text style={styles.phaseNumber}>{section.number}</Text> : null}
              <Text accessibilityRole="header" style={styles.phaseTitle}>
                {section.title}
              </Text>
              <Text style={styles.phaseProgress}>
                {section.completed}/{section.total}
              </Text>
            </View>
          ) : null}
          {section.rows.map(({ task, number, depth }, index) => (
            <ComposerTrackRow key={task.id ?? `${index}:${task.text}`}>
              <View style={styles.taskRow(depth)}>
                <TaskListRow task={task} number={number} numberOfLines={0} />
              </View>
            </ComposerTrackRow>
          ))}
        </View>
      ))}
    </ComposerTrackPill>
  );
});

const styles = StyleSheet.create((theme) => ({
  // The task row draws its own icon and text; this only lets it span the shared row frame.
  // Basis stays `auto` so the text's width reaches the panel's measurement — see track.tsx.
  taskRow: (depth: number) => ({
    flexGrow: 1,
    flexShrink: 1,
    flexBasis: "auto",
    minWidth: 0,
    paddingLeft: Math.min(depth, 4) * theme.spacing[3],
  }),
  phaseHeader: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
    paddingHorizontal: theme.spacing[3],
    paddingTop: theme.spacing[2],
    paddingBottom: theme.spacing[1],
  },
  phaseNumber: {
    minWidth: 24,
    textAlign: "right",
    color: theme.colors.foregroundExtraMuted,
    fontFamily: theme.fontFamily.mono,
    fontSize: theme.fontSize.sm,
  },
  phaseTitle: {
    flexGrow: 1,
    flexShrink: 1,
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    fontWeight: theme.fontWeight.medium,
  },
  phaseProgress: {
    color: theme.colors.foregroundExtraMuted,
    fontSize: theme.fontSize.sm,
  },
}));

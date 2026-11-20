import { Text, View } from "react-native";
import { ChevronRight } from "lucide-react-native";
import { StyleSheet, withUnistyles } from "react-native-unistyles";
import { ScreenTitle } from "@/components/headers/screen-title";
import { extraMutedIconColorMapping } from "@/components/ui/icon-color";

const ThemedChevronRight = withUnistyles(ChevronRight);

/**
 * The wide header's title: the project the workspace belongs to, then the workspace itself.
 *
 * The workspace is where you are, so it keeps the title weight and gives up its width last; the
 * project is the way back up and reads muted. A project name that only repeats the workspace name
 * is left out rather than shown as a crumb that leads nowhere new.
 */
export function WorkspaceHeaderBreadcrumb({
  project,
  workspace,
  showProject,
}: {
  project: string;
  workspace: string;
  showProject: boolean;
}) {
  return (
    <View style={styles.row}>
      {showProject ? (
        <>
          <Text testID="workspace-header-subtitle" style={styles.project} numberOfLines={1}>
            {project}
          </Text>
          <View style={styles.separator} testID="workspace-header-breadcrumb-separator">
            <ThemedChevronRight size={14} uniProps={extraMutedIconColorMapping} />
          </View>
        </>
      ) : null}
      <ScreenTitle testID="workspace-header-title">{workspace}</ScreenTitle>
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  row: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[1],
    minWidth: 0,
    flexShrink: 1,
    overflow: "hidden",
  },
  // Shrinks well before the workspace name does: a clipped project is still recognizable.
  project: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.base,
    flexShrink: 4,
    minWidth: 0,
  },
  separator: {
    flexShrink: 0,
  },
}));

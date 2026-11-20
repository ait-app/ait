import { Text, View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { HostBadge } from "@/hosts/host-badge";
import { useHostBadges } from "@/hosts/use-host-badges";

/**
 * Which project the workspace belongs to, and which machine it runs on.
 *
 * The compact header puts both on their own line under the workspace name: it is the only thing on
 * screen that says where the workspace lives, because the sidebar that normally carries the host
 * badge is closed. It still follows the host's own badge setting, so a purely local setup stays
 * quiet. The project is shown even when it only repeats the workspace name, since the line exists
 * for the host anyway. Wide layouts use `WorkspaceHeaderBreadcrumb` instead.
 */
export function WorkspaceHeaderProjectRow({
  subtitle,
  serverId,
}: {
  subtitle: string;
  serverId: string;
}) {
  const hostBadge = useHostBadges({ enabled: true }).get(serverId) ?? null;
  return (
    <View style={styles.row}>
      <Text testID="workspace-header-subtitle" style={styles.project} numberOfLines={1}>
        {subtitle}
      </Text>
      {hostBadge ? (
        <>
          <Text style={styles.separator}>·</Text>
          <HostBadge badge={hostBadge} />
        </>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  // The compact header stretches this row to its available title width. Within that width,
  // the project truncates and HostBadge yields its label while retaining its fixed icon.
  row: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[1.5],
    minWidth: 0,
    flexShrink: 1,
  },
  project: {
    color: theme.colors.foregroundMuted,
    fontSize: {
      xs: theme.fontSize.sm,
      md: theme.fontSize.base,
    },
    flexShrink: 1,
    minWidth: 0,
  },
  separator: {
    color: theme.colors.foregroundExtraMuted,
    fontSize: theme.fontSize.sm,
    flexShrink: 0,
  },
}));

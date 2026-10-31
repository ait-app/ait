import { memo, useCallback, useEffect, useMemo, useState } from "react";
import { Pressable, Text, View, type PressableStateCallbackType } from "react-native";
import { StyleSheet, withUnistyles } from "react-native-unistyles";
import {
  ChevronDown,
  ChevronRight,
  FolderGit2,
  Monitor,
  Users,
  Waypoints,
} from "lucide-react-native";
import type { HubShare } from "@ait/client/internal/hub";
import { useHubServices } from "@/runtime/hub-service-store";
import {
  getHostRuntimeStore,
  sharedResourceServerId,
  useHostRuntimeSnapshot,
} from "@/runtime/host-runtime";
import {
  useWorkspaceStructure,
  useWorkspace,
  useHasHydratedWorkspaces,
} from "@/stores/session-store-hooks";
import { navigateToWorkspace } from "@/stores/navigation-active-workspace-store";
import type { WorkspaceStructureProject } from "@/projects/workspace-structure";
import type { Theme } from "@/styles/theme";
import { useTranslation } from "react-i18next";

const ThemedChevronDown = withUnistyles(ChevronDown);
const ThemedChevronRight = withUnistyles(ChevronRight);
const ThemedFolderGit2 = withUnistyles(FolderGit2);
const ThemedMonitor = withUnistyles(Monitor);
const ThemedUsers = withUnistyles(Users);
const ThemedWaypoints = withUnistyles(Waypoints);
const mutedColor = (theme: Theme) => ({ color: theme.colors.foregroundMuted });

function ownerLabel(share: HubShare): string {
  return (
    share.owner.displayName?.trim() ||
    (share.owner.username ? `@${share.owner.username}` : share.hostName)
  );
}

function rowStyle({ hovered, pressed }: PressableStateCallbackType & { hovered?: boolean }) {
  return [styles.row, (hovered || pressed) && styles.rowActive];
}

function SharedWorkspaceRow({
  share,
  serverId,
  workspaceId,
  fallbackName,
  depth,
}: {
  share: HubShare;
  serverId: string;
  workspaceId: string;
  fallbackName: string;
  depth: number;
}) {
  const workspace = useWorkspace(serverId, workspaceId);
  const handlePress = useCallback(() => {
    navigateToWorkspace({ serverId, workspaceId });
  }, [serverId, workspaceId]);

  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={`${ownerLabel(share)} · ${workspace?.name ?? fallbackName}`}
      onPress={handlePress}
      style={rowStyle}
      testID={`shared-workspace-${share.shareId}-${workspaceId}`}
    >
      <View style={[styles.indent, { width: depth * 16 }]} />
      <ThemedWaypoints size={14} uniProps={mutedColor} />
      <Text numberOfLines={1} style={styles.rowLabel}>
        {workspace?.name ?? fallbackName}
      </Text>
      <View style={styles.statusDot} />
    </Pressable>
  );
}

function projectWorkspaceId(serverId: string, workspaceKey: string): string {
  return workspaceKey.slice(serverId.length + 1);
}

function SharedProject({
  share,
  serverId,
  project,
  depth,
  root,
}: {
  share: HubShare;
  serverId: string;
  project: WorkspaceStructureProject;
  depth: number;
  root?: boolean;
}) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(true);
  const toggle = useCallback(() => setExpanded((value) => !value), []);
  const Chevron = expanded ? ThemedChevronDown : ThemedChevronRight;

  return (
    <View>
      <Pressable
        accessibilityRole="button"
        accessibilityState={{ expanded }}
        onPress={toggle}
        style={rowStyle}
        testID={`shared-project-${share.shareId}-${project.viewKey}`}
      >
        <View style={[styles.indent, { width: depth * 16 }]} />
        <Chevron size={12} uniProps={mutedColor} />
        <ThemedFolderGit2 size={14} uniProps={mutedColor} />
        <Text numberOfLines={1} style={styles.rowLabel}>
          {root ? `${ownerLabel(share)} · ${project.projectName}` : project.projectName}
        </Text>
        {root ? <Text style={styles.scopeBadge}>{t("sharing.scope.project")}</Text> : null}
      </Pressable>
      {expanded
        ? project.workspaceKeys.map((workspaceKey) => {
            const workspaceId = projectWorkspaceId(serverId, workspaceKey);
            return (
              <SharedWorkspaceRow
                key={workspaceKey}
                share={share}
                serverId={serverId}
                workspaceId={workspaceId}
                fallbackName={share.displayName}
                depth={depth + 2}
              />
            );
          })
        : null}
    </View>
  );
}

function SharedRoot({
  share,
  projects,
}: {
  share: HubShare;
  projects: WorkspaceStructureProject[];
}) {
  const { t } = useTranslation();
  const serverId = sharedResourceServerId(share.shareId);
  const [expanded, setExpanded] = useState(true);
  const toggle = useCallback(() => setExpanded((value) => !value), []);
  const Chevron = expanded ? ThemedChevronDown : ThemedChevronRight;

  if (share.resourceKind === "workspace") {
    return (
      <SharedWorkspaceRow
        share={share}
        serverId={serverId}
        workspaceId={share.resourceKey}
        fallbackName={`${ownerLabel(share)} · ${share.displayName}`}
        depth={0}
      />
    );
  }

  if (share.resourceKind === "project") {
    const project = projects[0];
    if (project)
      return <SharedProject share={share} serverId={serverId} project={project} depth={0} root />;
  }

  return (
    <View>
      <Pressable
        accessibilityRole="button"
        accessibilityState={{ expanded }}
        onPress={toggle}
        style={rowStyle}
        testID={`shared-node-${share.shareId}`}
      >
        <Chevron size={12} uniProps={mutedColor} />
        <ThemedMonitor size={14} uniProps={mutedColor} />
        <Text numberOfLines={1} style={styles.rowLabel}>
          {ownerLabel(share)} · {share.hostName}
        </Text>
        <Text style={styles.scopeBadge}>
          {share.resourceKind === "node" ? t("sharing.scope.node") : t("sharing.scope.project")}
        </Text>
      </Pressable>
      {expanded ? (
        projects.length ? (
          projects.map((project) => (
            <SharedProject
              key={project.viewKey}
              share={share}
              serverId={serverId}
              project={project}
              depth={1}
            />
          ))
        ) : (
          <SharedDirectoryStatus share={share} serverId={serverId} />
        )
      ) : null}
    </View>
  );
}

function SharedDirectoryStatus({ share, serverId }: { share: HubShare; serverId: string }) {
  const { t } = useTranslation();
  const snapshot = useHostRuntimeSnapshot(serverId);
  const hydrated = useHasHydratedWorkspaces(serverId);
  const retry = useCallback(() => {
    void getHostRuntimeStore()
      .refreshWorkspaceDirectory({ serverId })
      .catch(() => undefined);
  }, [serverId]);

  if (share.state === "reconnecting" || snapshot?.connectionStatus === "offline") {
    return <Text style={styles.loading}>{t("sharing.offline")}</Text>;
  }
  if (snapshot?.workspaceDirectoryError || snapshot?.connectionStatus === "error") {
    return (
      <View>
        <Text style={styles.loading}>{t("sharing.listError")}</Text>
        {snapshot?.connectionStatus === "online" ? (
          <Pressable accessibilityRole="button" onPress={retry} style={rowStyle}>
            <Text style={styles.loading}>{t("common.actions.retry")}</Text>
          </Pressable>
        ) : null}
      </View>
    );
  }
  return (
    <Text style={styles.loading}>{t(hydrated ? "sharing.emptyResources" : "sharing.loading")}</Text>
  );
}

export const SharedResourceList = memo(function SharedResourceList({
  showHeader = true,
}: {
  showHeader?: boolean;
}) {
  const { t } = useTranslation();
  const shares = useHubServices((state) => state.snapshot?.shares ?? null);
  const serverIds = useMemo(
    () => (shares ?? []).map((share) => sharedResourceServerId(share.shareId)),
    [shares],
  );
  const { projects } = useWorkspaceStructure(serverIds);

  useEffect(() => {
    const runtime = getHostRuntimeStore();
    const releases = serverIds.map((serverId) => runtime.acquireDirectoryDemand(serverId));
    return () => releases.forEach((release) => release());
  }, [serverIds]);

  const byServer = useMemo(() => {
    const result = new Map<string, WorkspaceStructureProject[]>();
    for (const serverId of serverIds) {
      result.set(
        serverId,
        projects.filter((project) => project.hosts.some((host) => host.serverId === serverId)),
      );
    }
    return result;
  }, [projects, serverIds]);

  if (!shares?.length) return null;

  return (
    <View style={styles.section} testID="sidebar-shared-resources">
      {showHeader ? (
        <View style={styles.header}>
          <ThemedUsers size={13} uniProps={mutedColor} />
          <Text style={styles.headerLabel}>{t("sharing.section")}</Text>
        </View>
      ) : null}
      {shares.map((share) => (
        <SharedRoot
          key={share.shareId}
          share={share}
          projects={byServer.get(sharedResourceServerId(share.shareId)) ?? []}
        />
      ))}
    </View>
  );
});

const styles = StyleSheet.create((theme) => ({
  section: {
    paddingTop: theme.spacing[2],
    paddingBottom: theme.spacing[2],
  },
  header: {
    minHeight: 32,
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[1.5],
    paddingHorizontal: theme.spacing[2],
    color: theme.colors.foregroundMuted,
  },
  headerLabel: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
  row: {
    minHeight: 32,
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[1.5],
    paddingHorizontal: theme.spacing[2],
    borderRadius: theme.borderRadius.lg,
    color: theme.colors.foregroundMuted,
  },
  rowActive: {
    backgroundColor: theme.colors.surfaceSidebarHover,
    color: theme.colors.foreground,
  },
  rowLabel: {
    minWidth: 0,
    flex: 1,
    color: theme.colors.foreground,
    fontSize: theme.fontSize.sm,
  },
  indent: {
    height: 1,
    flexShrink: 0,
  },
  scopeBadge: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
  statusDot: {
    width: 6,
    height: 6,
    borderRadius: 3,
    backgroundColor: theme.colors.success,
  },
  loading: {
    paddingLeft: 46,
    paddingVertical: theme.spacing[1],
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
}));

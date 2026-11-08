import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Text, View } from "react-native";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import { Users } from "lucide-react-native";
import { useTranslation } from "react-i18next";
import type { ShareGroup, ShareResourceKind, ShareSetting } from "@ait/protocol/messages";
import { AdaptiveModalSheet, type SheetHeader } from "@/components/adaptive-modal-sheet";
import { Switch } from "@/components/ui/switch";
import { LoadingSpinner } from "@/components/ui/loading-spinner";
import { HostPicker } from "@/components/hosts/host-picker";
import { ComboboxTrigger } from "@/components/ui/combobox-trigger";
import { useLocalDaemonServerId } from "@/hooks/use-is-local-daemon";
import { isSharedResourceHost, useHostRuntimeClient, useHosts } from "@/runtime/host-runtime";
import { useActiveWorkspaceSelection } from "@/stores/navigation-active-workspace-store";

export interface ShareResourceTarget {
  serverId: string;
  resourceKind: ShareResourceKind;
  resourceKey: string;
  displayName: string;
  projectName?: string;
  projectHosts?: readonly { serverId: string; projectId: string }[];
}

function sameResource(setting: ShareSetting, target: ShareResourceTarget): boolean {
  return setting.resourceKind === target.resourceKind && setting.resourceKey === target.resourceKey;
}

export function ShareManagerSheet({
  visible,
  target,
  onClose,
}: {
  visible: boolean;
  target: ShareResourceTarget | null;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const header = useMemo<SheetHeader>(
    () => ({
      title: target
        ? t("sharing.shareTitle", { name: target.displayName })
        : t("sharing.resourceTitle"),
      subtitle: target ? (
        <Text style={styles.subtitle}>
          {target.resourceKind === "node"
            ? t("sharing.scopeHint.node")
            : target.resourceKind === "project"
              ? t("sharing.scopeHint.project")
              : t("sharing.scopeHint.workspace")}
        </Text>
      ) : null,
    }),
    [t, target],
  );

  return (
    <AdaptiveModalSheet
      visible={visible}
      header={header}
      onClose={onClose}
      desktopMaxWidth={480}
      testID="share-manager-sheet"
    >
      {visible && target ? (
        <ShareManagerContent
          key={JSON.stringify([target.serverId, target.resourceKind, target.resourceKey])}
          target={target}
        />
      ) : null}
    </AdaptiveModalSheet>
  );
}

function ShareManagerContent({ target: initialTarget }: { target: ShareResourceTarget }) {
  const { theme } = useUnistyles();
  const { t } = useTranslation();
  const hosts = useHosts();
  const activeWorkspace = useActiveWorkspaceSelection();
  const localServerId = useLocalDaemonServerId();
  const projectHosts = initialTarget.resourceKind === "project" ? initialTarget.projectHosts : null;
  const hostOptions = useMemo(
    () =>
      hosts.filter(
        (host) =>
          !isSharedResourceHost(host) &&
          projectHosts?.some((placement) => placement.serverId === host.serverId),
      ),
    [hosts, projectHosts],
  );
  const [selectedServerId, setSelectedServerId] = useState(() => {
    const preferredIds = [activeWorkspace?.serverId, localServerId, initialTarget.serverId];
    return (
      preferredIds.find((id) => hostOptions.some((host) => host.serverId === id)) ??
      hostOptions[0]?.serverId ??
      initialTarget.serverId
    );
  });
  const selectedHost =
    hostOptions.find((host) => host.serverId === selectedServerId) ?? hostOptions[0];
  const placement = projectHosts?.find((host) => host.serverId === selectedHost?.serverId);
  const target = useMemo(
    () =>
      placement
        ? { ...initialTarget, serverId: placement.serverId, resourceKey: placement.projectId }
        : initialTarget,
    [initialTarget, placement],
  );
  const client = useHostRuntimeClient(target.serverId);
  const host = hosts.find((candidate) => candidate.serverId === target.serverId);
  const isGuest = host ? isSharedResourceHost(host) : false;
  const [hostPickerOpen, setHostPickerOpen] = useState(false);
  const hostPickerAnchorRef = useRef<View>(null);
  const [groups, setGroups] = useState<ShareGroup[]>([]);
  const [settings, setSettings] = useState<ShareSetting[]>([]);
  const [loading, setLoading] = useState(true);
  const [pendingGroup, setPendingGroup] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const openHostPicker = useCallback(() => setHostPickerOpen(true), []);
  const selectHost = useCallback(
    (serverId: string) => {
      if (pendingGroup === null) setSelectedServerId(serverId);
    },
    [pendingGroup],
  );

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    setGroups([]);
    setSettings([]);
    if (isGuest) {
      setError(t("sharing.ownerOnly"));
      setLoading(false);
      return;
    }
    if (!client) {
      setError(t("sharing.hostUnavailable"));
      setLoading(false);
      return;
    }
    void (async () => {
      try {
        const result = await client.listShares();
        if (cancelled) return;
        setGroups(result.groups);
        setSettings(result.shares.filter((setting) => sameResource(setting, target)));
      } catch (cause) {
        if (cancelled) return;
        const code = cause && typeof cause === "object" && "code" in cause ? cause.code : null;
        setError(
          t(
            code === "forbidden"
              ? "sharing.ownerOnly"
              : code === "not_implemented"
                ? "sharing.unsupported"
                : "sharing.loadError",
          ),
        );
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [client, isGuest, t, target]);

  const toggleGroup = useCallback(
    async (group: ShareGroup) => {
      if (!client || !target || isGuest || pendingGroup) return;
      const existing = settings.find(
        (setting) => setting.groupId === group.groupId && setting.state !== "revoked",
      );
      setPendingGroup(group.groupId);
      setError(null);
      try {
        if (existing) {
          await client.removeShare({
            resourceKind: target.resourceKind,
            resourceKey: target.resourceKey,
            groupId: group.groupId,
          });
          setSettings((current) => current.filter((setting) => setting.groupId !== group.groupId));
        } else {
          const result = await client.setShare({
            resourceKind: target.resourceKind,
            resourceKey: target.resourceKey,
            groupId: group.groupId,
            displayName: target.displayName,
            projectName: target.projectName,
          });
          setSettings((current) => [
            ...current.filter((setting) => setting.groupId !== group.groupId),
            result.share,
          ]);
        }
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : t("sharing.updateError"));
      } finally {
        setPendingGroup(null);
      }
    },
    [client, isGuest, pendingGroup, settings, t, target],
  );

  return (
    <View style={styles.body}>
      <Text style={styles.description}>{t("sharing.intro")}</Text>
      {hostOptions.length > 0 ? (
        <View style={styles.hostField}>
          <Text style={styles.groupName}>{t("hostPicker.host")}</Text>
          <HostPicker
            hosts={hostOptions}
            value={target.serverId}
            onSelect={selectHost}
            open={hostPickerOpen && pendingGroup === null}
            onOpenChange={setHostPickerOpen}
            anchorRef={hostPickerAnchorRef}
            searchable
            hostOptionTestID={(serverId) => `share-host-${serverId}`}
          >
            <ComboboxTrigger
              ref={hostPickerAnchorRef}
              block
              style={styles.hostTrigger}
              onPress={openHostPicker}
              disabled={pendingGroup !== null}
              accessibilityRole="button"
              accessibilityLabel={t("hostPicker.host")}
              testID="share-host-picker"
            >
              <Text style={styles.hostName} numberOfLines={1}>
                {selectedHost?.label}
              </Text>
            </ComboboxTrigger>
          </HostPicker>
          <Text style={styles.groupMeta}>{t("sharing.projectHostHint")}</Text>
        </View>
      ) : null}
      {loading ? (
        <View style={styles.loading}>
          <LoadingSpinner size="small" color={theme.colors.foregroundMuted} />
        </View>
      ) : groups.length ? (
        <View style={styles.groupList}>
          {groups.map((group) => {
            const setting = settings.find(
              (candidate) => candidate.groupId === group.groupId && candidate.state !== "revoked",
            );
            const selected = Boolean(setting);
            return (
              <View key={group.groupId} style={styles.groupRow}>
                <View style={styles.groupIcon}>
                  <Users size={16} color={theme.colors.foregroundMuted} />
                </View>
                <View style={styles.groupCopy}>
                  <Text style={styles.groupName}>{group.name}</Text>
                  <Text style={styles.groupMeta}>
                    {setting?.state === "accepted"
                      ? t("sharing.status.shared")
                      : setting?.state === "rejected"
                        ? t("sharing.status.rejected", { code: setting.code ?? "rejected" })
                        : setting
                          ? t("sharing.status.waiting")
                          : group.myRole === "admin"
                            ? t("sharing.status.groupAdmin")
                            : t("sharing.status.groupMember")}
                  </Text>
                </View>
                {pendingGroup === group.groupId ? (
                  <LoadingSpinner size="small" color={theme.colors.foregroundMuted} />
                ) : null}
                <Switch
                  value={selected}
                  disabled={pendingGroup !== null}
                  onValueChange={() => void toggleGroup(group)}
                  accessibilityLabel={group.name}
                  testID={`share-group-${group.groupId}`}
                />
              </View>
            );
          })}
        </View>
      ) : !error ? (
        <View style={styles.empty}>
          <Text style={styles.emptyTitle}>{t("sharing.noGroupsTitle")}</Text>
          <Text style={styles.emptyText}>{t("sharing.noGroupsHint")}</Text>
        </View>
      ) : null}
      {error ? <Text style={styles.error}>{error}</Text> : null}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  body: {
    gap: theme.spacing[4],
    paddingBottom: theme.spacing[2],
  },
  subtitle: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
  description: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.base,
    lineHeight: 20,
  },
  hostField: {
    gap: theme.spacing[2],
  },
  hostTrigger: {
    padding: theme.spacing[3],
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: theme.borderRadius.lg,
    backgroundColor: theme.colors.surface2,
  },
  hostName: {
    flex: 1,
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
  },
  loading: {
    minHeight: 120,
    alignItems: "center",
    justifyContent: "center",
  },
  groupList: {
    overflow: "hidden",
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: theme.borderRadius.xl,
  },
  groupRow: {
    minHeight: 64,
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[3],
    paddingHorizontal: theme.spacing[3],
    paddingVertical: theme.spacing[2],
    borderBottomWidth: 1,
    borderBottomColor: theme.colors.border,
  },
  groupIcon: {
    width: 32,
    height: 32,
    alignItems: "center",
    justifyContent: "center",
    borderRadius: theme.borderRadius.lg,
    backgroundColor: theme.colors.surface3,
  },
  groupCopy: {
    minWidth: 0,
    flex: 1,
    gap: 2,
  },
  groupName: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
  },
  groupMeta: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
  empty: {
    gap: theme.spacing[1],
    padding: theme.spacing[4],
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: theme.borderRadius.xl,
    backgroundColor: theme.colors.surface2,
  },
  emptyTitle: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
  },
  emptyText: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    lineHeight: 18,
  },
  error: {
    color: theme.colors.destructive,
    fontSize: theme.fontSize.sm,
  },
}));

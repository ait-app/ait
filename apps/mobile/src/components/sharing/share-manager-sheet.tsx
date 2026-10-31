import { useCallback, useEffect, useMemo, useState } from "react";
import { Text, View } from "react-native";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import { Users } from "lucide-react-native";
import { useTranslation } from "react-i18next";
import type { ShareGroup, ShareResourceKind, ShareSetting } from "@ait/protocol/messages";
import { AdaptiveModalSheet, type SheetHeader } from "@/components/adaptive-modal-sheet";
import { Switch } from "@/components/ui/switch";
import { LoadingSpinner } from "@/components/ui/loading-spinner";
import { isSharedResourceHost, useHostRuntimeClient, useHosts } from "@/runtime/host-runtime";

export interface ShareResourceTarget {
  serverId: string;
  resourceKind: ShareResourceKind;
  resourceKey: string;
  displayName: string;
  projectName?: string;
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
  const { theme } = useUnistyles();
  const { t } = useTranslation();
  const client = useHostRuntimeClient(target?.serverId ?? "");
  const hosts = useHosts();
  const host = hosts.find((candidate) => candidate.serverId === target?.serverId);
  const isGuest = host ? isSharedResourceHost(host) : false;
  const [groups, setGroups] = useState<ShareGroup[]>([]);
  const [settings, setSettings] = useState<ShareSetting[]>([]);
  const [loading, setLoading] = useState(false);
  const [pendingGroup, setPendingGroup] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
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

  const load = useCallback(async () => {
    if (!client || !target) return;
    setLoading(true);
    setError(null);
    setGroups([]);
    setSettings([]);
    if (isGuest) {
      setError(t("sharing.ownerOnly"));
      setLoading(false);
      return;
    }
    try {
      const result = await client.listShares();
      setGroups(result.groups);
      setSettings(result.shares.filter((setting) => sameResource(setting, target)));
    } catch (cause) {
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
      setLoading(false);
    }
  }, [client, isGuest, t, target]);

  useEffect(() => {
    if (visible) void load();
  }, [load, visible]);

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
    <AdaptiveModalSheet
      visible={visible}
      header={header}
      onClose={onClose}
      desktopMaxWidth={480}
      testID="share-manager-sheet"
    >
      <View style={styles.body}>
        <Text style={styles.description}>{t("sharing.intro")}</Text>
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
    </AdaptiveModalSheet>
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

import { useCallback, useEffect, useMemo, useState } from "react";
import { Pressable, ScrollView, Text, View } from "react-native";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import { FolderGit2, Monitor, RefreshCw, Share2, Waypoints, X } from "lucide-react-native";
import type { ShareGroup, ShareSetting } from "@ait/protocol/messages";
import { MenuHeader } from "@/components/headers/menu-header";
import { SharedResourceList } from "@/components/sidebar/shared-resource-list";
import { Button } from "@/components/ui/button";
import { LoadingSpinner } from "@/components/ui/loading-spinner";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { getHostRuntimeStore, isSharedResourceHost, useHosts } from "@/runtime/host-runtime";
import { useHubServices } from "@/runtime/hub-service-store";
import { useTranslation } from "react-i18next";
import type { HubAccountSnapshot } from "@ait/client/internal/hub";

type SharedTab = "incoming" | "outgoing";
const EMPTY_INCOMING_SHARES = [] as const;

export function selectIncomingShares(snapshot: Pick<HubAccountSnapshot, "shares"> | null) {
  return snapshot?.shares ?? EMPTY_INCOMING_SHARES;
}

interface OutgoingShare {
  serverId: string;
  hostLabel: string;
  groupName: string;
  setting: ShareSetting;
}

function settingTitle(setting: ShareSetting, hostLabel: string): string {
  return (
    setting.displayName?.trim() || setting.projectName?.trim() || setting.resourceKey || hostLabel
  );
}

function ScopeIcon({ kind, color }: { kind: ShareSetting["resourceKind"]; color: string }) {
  if (kind === "workspace") return <Waypoints size={16} color={color} />;
  if (kind === "project") return <FolderGit2 size={16} color={color} />;
  return <Monitor size={16} color={color} />;
}

export function SharedResourcesScreen() {
  const { theme } = useUnistyles();
  const { t } = useTranslation();
  const [tab, setTab] = useState<SharedTab>("incoming");
  const incoming = useHubServices((state) => selectIncomingShares(state.snapshot));
  const hosts = useHosts();
  const ownerHosts = useMemo(() => hosts.filter((host) => !isSharedResourceHost(host)), [hosts]);
  const [outgoing, setOutgoing] = useState<OutgoingShare[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [removing, setRemoving] = useState<string | null>(null);

  const loadOutgoing = useCallback(async () => {
    setLoading(true);
    setError(null);
    const runtime = getHostRuntimeStore();
    try {
      const results = await Promise.all(
        ownerHosts.map(async (host) => {
          const client = runtime.getClient(host.serverId);
          if (!client) return [];
          const result = await client.listShares().catch((cause: unknown) => {
            const code = cause && typeof cause === "object" && "code" in cause ? cause.code : null;
            if (code === "not_implemented") return null;
            throw cause;
          });
          if (!result) return [];
          const groups = new Map<string, ShareGroup>(
            result.groups.map((group) => [group.groupId, group]),
          );
          return result.shares
            .filter((setting) => setting.state !== "revoked")
            .map((setting) => ({
              serverId: host.serverId,
              hostLabel: host.label,
              groupName: groups.get(setting.groupId)?.name ?? setting.groupId,
              setting,
            }));
        }),
      );
      setOutgoing(results.flat());
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("sharing.listError"));
    } finally {
      setLoading(false);
    }
  }, [ownerHosts, t]);

  useEffect(() => {
    if (tab === "outgoing") void loadOutgoing();
  }, [loadOutgoing, tab]);

  const remove = useCallback(
    async (entry: OutgoingShare) => {
      const client = getHostRuntimeStore().getClient(entry.serverId);
      if (!client) return;
      setRemoving(entry.setting.settingId);
      setError(null);
      try {
        await client.removeShare({
          resourceKind: entry.setting.resourceKind,
          resourceKey: entry.setting.resourceKey,
          groupId: entry.setting.groupId,
        });
        setOutgoing((current) =>
          current.filter((candidate) => candidate.setting.settingId !== entry.setting.settingId),
        );
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : t("sharing.stopError"));
      } finally {
        setRemoving(null);
      }
    },
    [t],
  );

  const options = useMemo(
    () => [
      {
        value: "incoming" as const,
        label: t("sharing.sharedWithMeCount", { count: incoming.length }),
      },
      {
        value: "outgoing" as const,
        label: t("sharing.sharedByMeCount", { count: outgoing.length }),
      },
    ],
    [incoming.length, outgoing.length, t],
  );

  return (
    <View style={styles.container}>
      <MenuHeader title={t("sharing.title")} />
      <View style={styles.toolbar}>
        <SegmentedControl options={options} value={tab} onValueChange={setTab} size="sm" />
        {tab === "outgoing" ? (
          <Button
            variant="ghost"
            size="sm"
            leftIcon={RefreshCw}
            onPress={() => void loadOutgoing()}
            disabled={loading}
          >
            {t("sharing.refresh")}
          </Button>
        ) : null}
      </View>
      {error ? <Text style={styles.error}>{error}</Text> : null}
      <ScrollView contentContainerStyle={styles.content}>
        {tab === "incoming" ? (
          incoming.length ? (
            <SharedResourceList showHeader={false} />
          ) : (
            <View style={styles.empty}>
              <Share2 size={28} color={theme.colors.foregroundMuted} />
              <Text style={styles.emptyTitle}>{t("sharing.emptyIncomingTitle")}</Text>
              <Text style={styles.emptyText}>{t("sharing.emptyIncomingHint")}</Text>
            </View>
          )
        ) : loading ? (
          <View style={styles.empty}>
            <LoadingSpinner size="large" color={theme.colors.foregroundMuted} />
          </View>
        ) : outgoing.length ? (
          <View style={styles.outgoingList}>
            {outgoing.map((entry) => (
              <View key={entry.setting.settingId} style={styles.outgoingRow}>
                <View style={styles.scopeIcon}>
                  <ScopeIcon
                    kind={entry.setting.resourceKind}
                    color={theme.colors.foregroundMuted}
                  />
                </View>
                <View style={styles.copy}>
                  <Text numberOfLines={1} style={styles.title}>
                    {settingTitle(entry.setting, entry.hostLabel)}
                  </Text>
                  <Text numberOfLines={1} style={styles.meta}>
                    {t(`sharing.scope.${entry.setting.resourceKind}`)} · {entry.groupName} ·{" "}
                    {entry.hostLabel}
                  </Text>
                </View>
                <Text style={styles.state}>
                  {entry.setting.state === "accepted" ? t("sharing.active") : entry.setting.state}
                </Text>
                <Pressable
                  accessibilityRole="button"
                  accessibilityLabel={t("sharing.stop")}
                  disabled={removing !== null}
                  onPress={() => void remove(entry)}
                  style={({ hovered, pressed }) => [
                    styles.remove,
                    (hovered || pressed) && styles.removeActive,
                  ]}
                >
                  {removing === entry.setting.settingId ? (
                    <LoadingSpinner size="small" color={theme.colors.foregroundMuted} />
                  ) : (
                    <X size={15} color={theme.colors.foregroundMuted} />
                  )}
                </Pressable>
              </View>
            ))}
          </View>
        ) : (
          <View style={styles.empty}>
            <Share2 size={28} color={theme.colors.foregroundMuted} />
            <Text style={styles.emptyTitle}>{t("sharing.emptyOutgoingTitle")}</Text>
            <Text style={styles.emptyText}>{t("sharing.emptyOutgoingHint")}</Text>
          </View>
        )}
      </ScrollView>
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  container: { flex: 1, backgroundColor: theme.colors.surface0 },
  toolbar: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: theme.spacing[3],
    paddingHorizontal: { xs: theme.spacing[3], md: theme.spacing[6] },
    paddingTop: theme.spacing[4],
  },
  content: {
    flexGrow: 1,
    paddingHorizontal: { xs: theme.spacing[3], md: theme.spacing[6] },
    paddingVertical: theme.spacing[4],
  },
  error: {
    marginHorizontal: theme.spacing[6],
    marginTop: theme.spacing[3],
    color: theme.colors.destructive,
    fontSize: theme.fontSize.sm,
  },
  empty: {
    flex: 1,
    minHeight: 260,
    alignItems: "center",
    justifyContent: "center",
    gap: theme.spacing[2],
  },
  emptyTitle: {
    marginTop: theme.spacing[2],
    color: theme.colors.foreground,
    fontSize: theme.fontSize.lg,
    fontWeight: theme.fontWeight.medium,
  },
  emptyText: {
    maxWidth: 460,
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.base,
    textAlign: "center",
  },
  outgoingList: {
    overflow: "hidden",
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: theme.borderRadius.xl,
  },
  outgoingRow: {
    minHeight: 68,
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[3],
    paddingHorizontal: theme.spacing[4],
    paddingVertical: theme.spacing[3],
    borderBottomWidth: 1,
    borderBottomColor: theme.colors.border,
  },
  scopeIcon: {
    width: 34,
    height: 34,
    alignItems: "center",
    justifyContent: "center",
    borderRadius: theme.borderRadius.lg,
    backgroundColor: theme.colors.surface2,
  },
  copy: { minWidth: 0, flex: 1, gap: 3 },
  title: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
  },
  meta: { color: theme.colors.foregroundMuted, fontSize: theme.fontSize.sm },
  state: { color: theme.colors.foregroundMuted, fontSize: theme.fontSize.sm },
  remove: {
    width: 30,
    height: 30,
    alignItems: "center",
    justifyContent: "center",
    borderRadius: theme.borderRadius.lg,
  },
  removeActive: { backgroundColor: theme.colors.surface2 },
}));

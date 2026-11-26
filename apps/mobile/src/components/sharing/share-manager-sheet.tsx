import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Text, View } from "react-native";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import { UserRound, Users } from "lucide-react-native";
import { useTranslation } from "react-i18next";
import type { ShareGroup, ShareResourceKind, ShareSetting } from "@ait/protocol/messages";
import { AdaptiveModalSheet, type SheetHeader } from "@/components/adaptive-modal-sheet";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { LoadingSpinner } from "@/components/ui/loading-spinner";
import { HostPicker } from "@/components/hosts/host-picker";
import { ComboboxTrigger } from "@/components/ui/combobox-trigger";
import { useLocalDaemonServerId } from "@/hooks/use-is-local-daemon";
import { isSharedResourceHost, useHostRuntimeClient, useHosts } from "@/runtime/host-runtime";
import { useAccountState } from "@/runtime/account-state";
import { openExternalUrl } from "@/utils/open-external-url";
import { refreshOutgoingShares } from "@/sharing/share-lists";
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
  const [loadState, setLoadState] = useState<"loading" | "loaded" | "failed">("loading");
  const [pendingAudience, setPendingAudience] = useState<string | null>(null);
  const center = useAccountState((state) => state.center);
  const [error, setError] = useState<string | null>(null);
  const direct = settings.find(
    (setting) => setting.audience === "people" && setting.state !== "revoked",
  );
  const pendingDirectId = direct?.state === "pending" ? direct.settingId : null;
  const openHostPicker = useCallback(() => setHostPickerOpen(true), []);
  const selectHost = useCallback(
    (serverId: string) => {
      if (pendingAudience === null) setSelectedServerId(serverId);
    },
    [pendingAudience],
  );

  useEffect(() => {
    let cancelled = false;
    setLoadState("loading");
    setError(null);
    setGroups([]);
    setSettings([]);
    if (isGuest) {
      setError(t("sharing.ownerOnly"));
      setLoadState("failed");
      return;
    }
    if (!client) {
      setError(t("sharing.hostUnavailable"));
      setLoadState("failed");
      return;
    }
    void (async () => {
      try {
        const result = await client.listShares();
        if (cancelled) return;
        setGroups(result.groups);
        setSettings(result.shares.filter((setting) => sameResource(setting, target)));
        setLoadState("loaded");
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
        setLoadState("failed");
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [client, isGuest, t, target]);

  useEffect(() => {
    if (!client || !pendingDirectId || pendingAudience) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const check = async () => {
      try {
        const result = await client.listShares();
        if (cancelled) return;
        const updated = result.shares.find((setting) => setting.settingId === pendingDirectId);
        if (updated?.state !== "pending") {
          setSettings((current) =>
            current.flatMap((setting) =>
              setting.settingId === pendingDirectId ? (updated ? [updated] : []) : [setting],
            ),
          );
          refreshOutgoingShares(target.serverId);
          return;
        }
      } catch {
        // A temporary disconnection can outlast a check.
      }
      if (!cancelled) timer = setTimeout(() => void check(), 1500);
    };
    timer = setTimeout(() => void check(), 1500);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [client, pendingAudience, pendingDirectId, target.serverId]);

  const toggleShare = useCallback(
    async (groupId?: string) => {
      if (!client || isGuest || pendingAudience) return;
      const matches = (setting: ShareSetting) => (setting.groupId ?? undefined) === groupId;
      const existing = settings.find((setting) => matches(setting) && setting.state !== "revoked");
      setPendingAudience(groupId ?? "people");
      setError(null);
      try {
        if (existing) {
          await client.removeShare({
            resourceKind: target.resourceKind,
            resourceKey: target.resourceKey,
            ...(groupId ? { groupId } : {}),
          });
          setSettings((current) => current.filter((setting) => !matches(setting)));
        } else {
          const result = await client.setShare({
            resourceKind: target.resourceKind,
            resourceKey: target.resourceKey,
            ...(groupId ? { groupId } : { audience: "people" }),
            displayName: target.displayName,
            projectName: target.projectName,
          });
          setSettings((current) => [
            ...current.filter((setting) => !matches(setting)),
            result.share,
          ]);
        }
        // The sidebar's Shared row and the Shared screen list this Host's shares too.
        refreshOutgoingShares(target.serverId);
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : t("sharing.updateError"));
      } finally {
        setPendingAudience(null);
      }
    },
    [client, isGuest, pendingAudience, settings, t, target],
  );

  const openInvitations = () => {
    if (direct?.state !== "accepted" || !direct.shareId) return;
    setError(null);
    const url = new URL(`/dash/shares/${encodeURIComponent(direct.shareId)}`, center);
    void openExternalUrl(url.href).catch(() => setError(t("sharing.openInvitationsError")));
  };

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
            open={hostPickerOpen && pendingAudience === null}
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
              disabled={pendingAudience !== null}
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
      {loadState === "loading" ? (
        <View style={styles.loading}>
          <LoadingSpinner size="small" color={theme.colors.foregroundMuted} />
        </View>
      ) : loadState === "loaded" ? (
        <View style={styles.groupList}>
          <View style={styles.groupRow}>
            <View style={styles.groupIcon}>
              <UserRound size={16} color={theme.colors.foregroundMuted} />
            </View>
            <View style={styles.groupCopy}>
              <Text style={styles.groupName}>{t("sharing.direct")}</Text>
              <Text style={styles.groupMeta}>
                {direct?.state === "accepted"
                  ? t("sharing.status.shared")
                  : direct?.state === "pending"
                    ? t("sharing.status.waiting")
                    : direct?.state === "rejected"
                      ? direct.code === "direct_shares_unsupported"
                        ? t("sharing.directUnsupported")
                        : t("sharing.status.rejected", { code: direct.code ?? "rejected" })
                      : t("sharing.directHint")}
              </Text>
            </View>
            {pendingAudience === "people" ? (
              <LoadingSpinner size="small" color={theme.colors.foregroundMuted} />
            ) : null}
            <Switch
              value={Boolean(direct)}
              disabled={pendingAudience !== null}
              onValueChange={() => void toggleShare()}
              accessibilityLabel={t("sharing.direct")}
            />
          </View>
          {direct?.state === "accepted" && direct.shareId ? (
            <View style={styles.invitationActions}>
              <Button variant="secondary" size="sm" onPress={openInvitations}>
                {t("sharing.openInvitations")}
              </Button>
            </View>
          ) : null}
        </View>
      ) : null}
      {loadState === "loaded" && groups.length ? (
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
                {pendingAudience === group.groupId ? (
                  <LoadingSpinner size="small" color={theme.colors.foregroundMuted} />
                ) : null}
                <Switch
                  value={selected}
                  disabled={pendingAudience !== null}
                  onValueChange={() => void toggleShare(group.groupId)}
                  accessibilityLabel={group.name}
                  testID={`share-group-${group.groupId}`}
                />
              </View>
            );
          })}
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
  invitationActions: {
    alignItems: "flex-start",
    padding: theme.spacing[3],
  },
  error: {
    color: theme.colors.destructive,
    fontSize: theme.fontSize.sm,
  },
}));

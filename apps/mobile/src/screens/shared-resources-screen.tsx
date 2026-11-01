import { useCallback, useEffect, useMemo, useState } from "react";
import { ScrollView, Text, View } from "react-native";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import { RefreshCw, X } from "lucide-react-native";
import type { ShareGroup, ShareSetting } from "@ait/protocol/messages";
import { MenuHeader } from "@/components/headers/menu-header";
import { Button } from "@/components/ui/button";
import { LoadingSpinner } from "@/components/ui/loading-spinner";
import { SettingsCard, SettingsRow, SettingsSection } from "@/components/settings";
import { Switch } from "@/components/ui/switch";
import { getHostRuntimeStore, isSharedResourceHost, useHosts } from "@/runtime/host-runtime";
import { useHubServices } from "@/runtime/hub-service-store";
import { useAccountState } from "@/runtime/account-state";
import { groupIncomingSharedHosts } from "@/runtime/shared-hosts";
import {
  isSharedHostEnabled,
  useSharedHostPreferences,
} from "@/stores/shared-host-preferences-store";
import { useTranslation } from "react-i18next";
import type { HubAccountSnapshot } from "@ait/client/internal/hub";

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

export function SharedResourcesScreen() {
  const { theme } = useUnistyles();
  const { t } = useTranslation();
  const incoming = useHubServices((state) => selectIncomingShares(state.snapshot));
  const incomingHosts = useMemo(() => groupIncomingSharedHosts(incoming), [incoming]);
  const center = useAccountState((state) => state.center);
  const accountId = useAccountState((state) => state.accountId);
  const disabledHostKeys = useSharedHostPreferences((state) => state.disabledHostKeys);
  const preferencesHydrated = useSharedHostPreferences((state) => state.hydrated);
  const setHostEnabled = useSharedHostPreferences((state) => state.setHostEnabled);
  const hosts = useHosts();
  const ownerHosts = useMemo(() => hosts.filter((host) => !isSharedResourceHost(host)), [hosts]);
  const [outgoing, setOutgoing] = useState<OutgoingShare[]>([]);
  const [loading, setLoading] = useState(true);
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
    void loadOutgoing();
  }, [loadOutgoing]);

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

  return (
    <View style={styles.container}>
      <MenuHeader title={t("sharing.title")} />
      <ScrollView>
        <View style={styles.content}>
          <SettingsSection
            title={t("sharing.sharedWithMeCount", { count: incomingHosts.length })}
            info={t("sharing.incomingHostHint")}
            testID="sharing-incoming"
          >
            <SettingsCard>
              {incomingHosts.length ? (
                incomingHosts.map((host) => (
                  <SettingsRow
                    key={host.hostId}
                    label={host.label}
                    hint={host.groupNames.join(" · ")}
                  >
                    <Switch
                      value={isSharedHostEnabled(
                        { disabledHostKeys },
                        { center, accountId },
                        host.hostId,
                      )}
                      disabled={!preferencesHydrated}
                      accessibilityLabel={t("sharing.showHostInHosts", { host: host.label })}
                      onValueChange={(enabled) =>
                        setHostEnabled({ center, accountId }, host.hostId, enabled)
                      }
                    />
                  </SettingsRow>
                ))
              ) : (
                <SettingsRow
                  label={t("sharing.emptyIncomingTitle")}
                  hint={t("sharing.emptyIncomingHint")}
                />
              )}
            </SettingsCard>
          </SettingsSection>
          <SettingsSection
            title={t("sharing.sharedByMeCount", { count: outgoing.length })}
            testID="sharing-outgoing"
            trailing={
              <Button
                variant="ghost"
                size="sm"
                leftIcon={RefreshCw}
                onPress={() => void loadOutgoing()}
                disabled={loading}
              >
                {t("sharing.refresh")}
              </Button>
            }
            flush
          >
            {error ? (
              <Text accessibilityRole="alert" style={styles.error}>
                {error}
              </Text>
            ) : null}
            {loading && outgoing.length === 0 ? (
              <SettingsCard>
                <View style={styles.loading}>
                  <LoadingSpinner size="small" color={theme.colors.foregroundMuted} />
                  <Text style={styles.meta}>{t("sharing.loading")}</Text>
                </View>
              </SettingsCard>
            ) : outgoing.length ? (
              <SettingsCard>
                {outgoing.map((entry) => (
                  <SettingsRow
                    key={entry.setting.settingId}
                    label={settingTitle(entry.setting, entry.hostLabel)}
                    hint={`${t(`sharing.scope.${entry.setting.resourceKind}`)} · ${entry.groupName} · ${entry.hostLabel}`}
                  >
                    <View style={styles.actions}>
                      <Text style={styles.meta}>
                        {entry.setting.state === "accepted"
                          ? t("sharing.active")
                          : entry.setting.state}
                      </Text>
                      <Button
                        variant="ghost"
                        size="sm"
                        leftIcon={X}
                        loading={removing === entry.setting.settingId}
                        disabled={removing !== null}
                        onPress={() => void remove(entry)}
                      >
                        {t("sharing.stop")}
                      </Button>
                    </View>
                  </SettingsRow>
                ))}
              </SettingsCard>
            ) : !error ? (
              <SettingsCard>
                <SettingsRow
                  label={t("sharing.emptyOutgoingTitle")}
                  hint={t("sharing.emptyOutgoingHint")}
                />
              </SettingsCard>
            ) : null}
          </SettingsSection>
        </View>
      </ScrollView>
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  container: { flex: 1, backgroundColor: theme.colors.surface0 },
  content: {
    padding: theme.spacing[4],
    paddingTop: theme.spacing[6],
    width: "100%",
    maxWidth: 720,
    alignSelf: "center",
  },
  error: { color: theme.colors.destructive, fontSize: theme.fontSize.sm },
  loading: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "center",
    gap: theme.spacing[2],
    padding: theme.spacing[4],
  },
  meta: { color: theme.colors.foregroundMuted, fontSize: theme.fontSize.sm },
  actions: { flexDirection: "row", alignItems: "center", gap: theme.spacing[2] },
}));

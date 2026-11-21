import { useCallback, useMemo, useState } from "react";
import { ScrollView, Text, View } from "react-native";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import { RefreshCw, X } from "lucide-react-native";
import type { ShareSetting } from "@ait/protocol/messages";
import { MenuHeader } from "@/components/headers/menu-header";
import { Button } from "@/components/ui/button";
import { LoadingSpinner } from "@/components/ui/loading-spinner";
import { SettingsCard, SettingsRow, SettingsSection } from "@/components/settings";
import { Switch } from "@/components/ui/switch";
import { getHostRuntimeStore } from "@/runtime/host-runtime";
import { useHubServices } from "@/runtime/hub-service-store";
import { useAccountState } from "@/runtime/account-state";
import { groupIncomingSharedHosts } from "@/runtime/shared-hosts";
import {
  forgetOutgoingShare,
  refreshOutgoingShares,
  selectIncomingShares,
  useOutgoingShares,
  type OutgoingShare,
} from "@/sharing/share-lists";
import {
  isSharedHostEnabled,
  useSharedHostPreferences,
} from "@/stores/shared-host-preferences-store";
import { useTranslation } from "react-i18next";

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
  const { shares: outgoing, loading, error: listError } = useOutgoingShares();
  const [stopError, setStopError] = useState<string | null>(null);
  const [removing, setRemoving] = useState<string | null>(null);
  const error = stopError ?? (listError ? listError.message || t("sharing.listError") : null);

  const refresh = useCallback(() => {
    setStopError(null);
    refreshOutgoingShares();
  }, []);

  const remove = useCallback(
    async (entry: OutgoingShare) => {
      const client = getHostRuntimeStore().getClient(entry.serverId);
      if (!client) return;
      setRemoving(entry.setting.settingId);
      setStopError(null);
      try {
        await client.removeShare({
          resourceKind: entry.setting.resourceKind,
          resourceKey: entry.setting.resourceKey,
          groupId: entry.setting.groupId,
        });
        await forgetOutgoingShare(entry.serverId, entry.setting.settingId);
      } catch (cause) {
        setStopError(cause instanceof Error ? cause.message : t("sharing.stopError"));
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
                onPress={refresh}
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

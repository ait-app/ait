import React, { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useRouter } from "expo-router";
import { Text, View } from "react-native";
import { SettingsCard, SettingsRow, SettingsSection } from "@/components/settings";
import { Button } from "@/components/ui/button";
import { useAccountState } from "@/runtime/account-state";
import { useHostRuntimeClient, useHostRuntimeIsConnected } from "@/runtime/host-runtime";
import {
  disconnectOnlineServiceHost,
  synchronizeOnlineServiceHost,
  useOnlineServiceHostSync,
} from "@/runtime/online-service-host-sync";
import { settingsStyles } from "@/styles/settings";
import type { HostProfile } from "@/types/host-connection";
import { buildSettingsSectionRoute } from "@/utils/host-routes";

export function OnlineServiceHostSection({ host }: { host: HostProfile }) {
  const { t } = useTranslation();
  const router = useRouter();
  const account = useAccountState();
  const client = useHostRuntimeClient(host.serverId);
  const connected = useHostRuntimeIsConnected(host.serverId);
  const sync = useOnlineServiceHostSync((state) => state.hosts[host.serverId]);
  const supported = client?.getLastServerInfoMessage()?.features?.onlineServiceSync === true;
  useEffect(() => {
    if (connected && supported) void synchronizeOnlineServiceHost(host.serverId, host.label);
  }, [connected, supported, host.serverId, host.label]);
  const loggedOut = account.status === "logged_out";
  const online = sync?.status?.status.online;
  const connecting = sync?.status?.status.connecting;
  const statusKey = !connected
    ? "hostDisconnected"
    : !supported
      ? "updateHost"
      : online
        ? "hostOnline"
        : connecting
          ? "hostConnecting"
          : "hostOffline";
  return (
    <SettingsSection
      title={t("onlineService.syncTitle")}
      info={t("onlineService.syncDescription")}
      testID="host-online-service-section"
    >
      <SettingsCard>
        <SettingsRow
          label={t("onlineService.title")}
          hint={loggedOut ? t("onlineService.signInFirst") : `${account.name} · ${account.center}`}
        >
          <Text style={settingsStyles.rowHint}>{t(`onlineService.${statusKey}`)}</Text>
          {loggedOut ? (
            <Button
              variant="outline"
              size="sm"
              onPress={() => router.push(buildSettingsSectionRoute("online-service"))}
              testID="host-online-service-sign-in"
            >
              {t("onlineService.signIn")}
            </Button>
          ) : null}
        </SettingsRow>
        <SettingsRow
          label={t("onlineService.syncHost")}
          hint={t("onlineService.syncHostHint", { name: host.label })}
          error={sync?.error ?? sync?.status?.status.error}
        >
          <View>
            <Button
              variant="outline"
              size="sm"
              disabled={
                (loggedOut && !sync?.enabled) ||
                !connected ||
                !supported ||
                sync?.busy ||
                Boolean((connecting || online) && !sync?.enabled)
              }
              onPress={() =>
                void (sync?.enabled
                  ? disconnectOnlineServiceHost(host.serverId)
                  : synchronizeOnlineServiceHost(host.serverId, host.label, true))
              }
              testID="host-online-service-sync"
            >
              {t(
                sync?.busy
                  ? "onlineService.syncing"
                  : sync?.enabled
                    ? "onlineService.stopSync"
                    : online
                      ? "onlineService.hostOnline"
                      : "onlineService.syncAction",
              )}
            </Button>
          </View>
        </SettingsRow>
      </SettingsCard>
    </SettingsSection>
  );
}

import React, { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useRouter } from "expo-router";
import { Text } from "react-native";
import { SettingsCard, SettingsRow, SettingsSection } from "@/components/settings";
import { Button } from "@/components/ui/button";
import { useAccountState } from "@/runtime/account-state";
import { useHostRuntimeClient, useHostRuntimeIsConnected } from "@/runtime/host-runtime";
import {
  disconnectOnlineServiceHost,
  synchronizeOnlineServiceHost,
  useOnlineServiceHostSync,
  type HostSyncState,
} from "@/runtime/online-service-host-sync";
import { settingsStyles } from "@/styles/settings";
import type { HostProfile } from "@/types/host-connection";
import { buildSettingsSectionRoute } from "@/utils/host-routes";

const actionLabels = {
  start: "onlineService.syncAction",
  stop: "onlineService.stopSync",
  signIn: "onlineService.signIn",
} as const;

function hostStatus(connected: boolean, supported: boolean, sync: HostSyncState | undefined) {
  if (!connected) return "hostDisconnected";
  // Old daemon status replies have no publication snapshot and cannot use these controls.
  if (!supported || (sync?.status && !sync.publication)) return "updateHost";
  const link = sync?.publication?.hub.status;
  if (!link) return sync?.busy ? "syncing" : "hostOffline";
  if (link.online) return "hostOnline";
  return link.connecting ? "hostConnecting" : "hostOffline";
}

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
  const status = hostStatus(connected, supported, sync);
  const action = sync?.enabled ? "stop" : account.status === "logged_out" ? "signIn" : "start";
  const actions = {
    start: () => void synchronizeOnlineServiceHost(host.serverId, host.label, true),
    stop: () => void disconnectOnlineServiceHost(host.serverId),
    signIn: () => router.push(buildSettingsSectionRoute("online-service")),
  };
  return (
    <SettingsSection
      title={t("onlineService.syncTitle")}
      info={t("onlineService.syncDescription")}
      testID="host-online-service-section"
    >
      <SettingsCard>
        <SettingsRow
          label={t("onlineService.title")}
          hint={
            sync?.enabled
              ? t("onlineService.managedHostHint")
              : account.status === "logged_out"
                ? t("onlineService.signInFirst")
                : `${account.name} · ${account.center}`
          }
        >
          <Text style={settingsStyles.rowHint}>{t(`onlineService.${status}`)}</Text>
        </SettingsRow>
        <SettingsRow
          label={t("onlineService.syncHost")}
          hint={t("onlineService.syncHostHint", { name: host.label })}
          error={sync?.error ?? sync?.status?.status.error}
        >
          <Button
            variant="outline"
            size="sm"
            disabled={sync?.busy || status === "hostDisconnected" || status === "updateHost"}
            onPress={actions[action]}
            testID="host-online-service-sync"
          >
            {t(sync?.busy ? "onlineService.syncing" : actionLabels[action])}
          </Button>
        </SettingsRow>
      </SettingsCard>
    </SettingsSection>
  );
}

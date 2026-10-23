import React, { useEffect, useState } from "react";
import { Text } from "react-native";
import { useTranslation } from "react-i18next";
import type { AccountInfo } from "@ait/client/internal/account-session";
import { AccountHostPanel } from "@/components/account-host-panel";
import { SettingsCard, SettingsRow, SettingsSection } from "@/components/settings";
import { loadAccountInfo, useAccountState } from "@/runtime/account-state";
import { useAppVisible } from "@/hooks/use-app-visible";
import { settingsStyles } from "@/styles/settings";

function AccountInformation({ refreshKey }: { refreshKey: number }) {
  const { t, i18n } = useTranslation();
  const visible = useAppVisible();
  const [info, setInfo] = useState<AccountInfo | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (!visible) return;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const refresh = async () => {
      try {
        const next = await loadAccountInfo();
        if (!disposed) {
          setInfo(next);
          setFailed(false);
        }
      } catch {
        if (!disposed) {
          setInfo(null);
          setFailed(true);
        }
      } finally {
        if (!disposed) timer = setTimeout(() => void refresh(), 15_000);
      }
    };
    void refresh();
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, [visible, refreshKey]);

  const unavailable = t("onlineService.unavailable");
  const expiresAt = info
    ? info.expiresAt === null
      ? t("onlineService.neverExpires")
      : new Date(info.expiresAt).toLocaleString(i18n.language)
    : "—";
  const onlineClients = info ? (info.onlineClients ?? unavailable) : "—";
  const onlineNodes = info ? (info.onlineNodes ?? unavailable) : "—";

  return (
    <SettingsSection
      title={t("onlineService.accountInfo")}
      info={t("onlineService.accountInfoHint")}
      testID="settings-account-info"
    >
      <SettingsCard>
        <SettingsRow label={t("onlineService.loginName")} testID="account-info-login-name">
          <Text selectable style={settingsStyles.rowTitle}>
            {info?.loginName ?? "—"}
          </Text>
        </SettingsRow>
        <SettingsRow label={t("onlineService.accountExpiry")} testID="account-info-expiry">
          <Text selectable style={settingsStyles.rowTitle}>
            {expiresAt}
          </Text>
        </SettingsRow>
        <SettingsRow label={t("onlineService.onlineClients")} testID="account-info-clients">
          <Text style={settingsStyles.rowTitle}>{onlineClients}</Text>
        </SettingsRow>
        <SettingsRow label={t("onlineService.onlineNodes")} testID="account-info-nodes">
          <Text style={settingsStyles.rowTitle}>{onlineNodes}</Text>
        </SettingsRow>
      </SettingsCard>
      {failed ? (
        <Text accessibilityRole="alert" style={settingsStyles.rowError}>
          {t("onlineService.accountInfoFailed")}
        </Text>
      ) : null}
    </SettingsSection>
  );
}

export function OnlineServiceSection() {
  const { t } = useTranslation();
  const account = useAccountState();
  const [refreshKey, setRefreshKey] = useState(0);
  return (
    <>
      <SettingsSection
        title={t("onlineService.accountTitle")}
        info={t("onlineService.loginDescription")}
        testID="settings-online-service"
      >
        <AccountHostPanel onRefresh={() => setRefreshKey((value) => value + 1)} />
      </SettingsSection>
      {account.status !== "logged_out" ? (
        <AccountInformation
          key={JSON.stringify([account.center, account.accountId, account.name])}
          refreshKey={refreshKey}
        />
      ) : null}
    </>
  );
}

import React, { useEffect, useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import type { AccountInfo } from "@ait/client/internal/account-session";
import { AccountHostPanel } from "@/components/account-host-panel";
import { SettingsCard, SettingsRow, SettingsSection } from "@/components/settings";
import { loadAccountInfo, useAccountState } from "@/runtime/account-state";
import { supportsBrowserAccount } from "@/runtime/browser-account";
import { useAppVisible } from "@/hooks/use-app-visible";
import { settingsStyles } from "@/styles/settings";

function AccountInformation({ refreshKey }: { refreshKey: number }) {
  const { t, i18n } = useTranslation();
  const account = useAccountState();
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

  const expiresAt = info
    ? info.expiresAt === null
      ? t("onlineService.neverExpires")
      : new Date(info.expiresAt).toLocaleString(i18n.language)
    : "—";

  return (
    <SettingsSection title={t("onlineService.accountInfo")} testID="settings-account-info" flush>
      <SettingsCard>
        <SettingsRow label={t("onlineService.loginName")} testID="account-info-login-name">
          <Text selectable style={styles.value}>
            {info?.loginName ?? "—"}
          </Text>
        </SettingsRow>
        <SettingsRow label={t("onlineService.accountExpiry")} testID="account-info-expiry">
          <Text selectable style={styles.value}>
            {expiresAt}
          </Text>
        </SettingsRow>
        {!supportsBrowserAccount() ? (
          <SettingsRow label={t("onlineService.serviceUrl")}>
            <Text selectable style={styles.value}>
              {account.center}
            </Text>
          </SettingsRow>
        ) : null}
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
  const account = useAccountState();
  const [refreshKey, setRefreshKey] = useState(0);
  return (
    <View testID="settings-online-service">
      <AccountHostPanel onRefresh={() => setRefreshKey((value) => value + 1)}>
        {account.status !== "logged_out" ? (
          <AccountInformation
            key={JSON.stringify([account.center, account.accountId, account.name])}
            refreshKey={refreshKey}
          />
        ) : null}
      </AccountHostPanel>
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  value: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.base,
    flexShrink: 1,
  },
}));

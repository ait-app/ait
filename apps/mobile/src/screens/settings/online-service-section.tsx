import React, { useEffect, useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { ExternalLink } from "lucide-react-native";
import type { AccountInfo } from "@ait/client/internal/account-session";
import { AccountHostPanel } from "@/components/account-host-panel";
import { SettingsCard, SettingsRow, SettingsSection } from "@/components/settings";
import { Button } from "@/components/ui/button";
import { loadAccountInfo, useAccountState } from "@/runtime/account-state";
import { supportsBrowserAccount } from "@/runtime/browser-account";
import { useAppVisible } from "@/hooks/use-app-visible";
import { settingsStyles } from "@/styles/settings";
import { openExternalUrl } from "@/utils/open-external-url";

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
  const { t } = useTranslation();
  const account = useAccountState();
  const [refreshKey, setRefreshKey] = useState(0);
  const [portalOpenFailed, setPortalOpenFailed] = useState(false);

  const openLoginPortal = async () => {
    setPortalOpenFailed(false);
    try {
      await openExternalUrl("https://ait-login.authing.cn/u");
    } catch {
      setPortalOpenFailed(true);
    }
  };

  return (
    <View style={styles.section} testID="settings-online-service">
      <AccountHostPanel onRefresh={() => setRefreshKey((value) => value + 1)}>
        {account.status !== "logged_out" ? (
          <AccountInformation
            key={JSON.stringify([account.center, account.accountId, account.name])}
            refreshKey={refreshKey}
          />
        ) : null}
      </AccountHostPanel>
      {account.status !== "logged_out" ? (
        <SettingsCard>
          <SettingsRow
            label={t("onlineService.loginPortal")}
            hint={t("onlineService.loginPortalHint")}
            error={portalOpenFailed ? t("onlineService.loginPortalOpenFailed") : null}
          >
            <Button
              variant="outline"
              size="sm"
              leftIcon={ExternalLink}
              testID="account-login-portal"
              onPress={() => void openLoginPortal()}
            >
              {t("onlineService.openInBrowser")}
            </Button>
          </SettingsRow>
        </SettingsCard>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  section: {
    gap: theme.spacing[6],
  },
  value: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.base,
    flexShrink: 1,
  },
}));

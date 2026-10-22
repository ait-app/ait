import { useCallback, useEffect, useState } from "react";
import { AppState, Linking, Text, View } from "react-native";
import * as Notifications from "expo-notifications";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { SettingsSection } from "@/components/settings/headings/settings-section";
import { settingsStyles } from "@/styles/settings";
import {
  ensureNativeNotificationPermission,
  isPushPermissionGranted,
} from "@/push-notifications/internal/permissions";
import { refreshPushRegistrations } from "@/push-notifications/internal/subscriptions";
import { sendOsNotification } from "@/utils/os-notifications";

export function NativeNotificationsSection() {
  const { t } = useTranslation();
  const [permission, setPermission] = useState<Notifications.NotificationPermissionsStatus | null>(
    null,
  );
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    const refresh = () => {
      void Notifications.getPermissionsAsync()
        .then((value) => {
          if (active) setPermission(value);
        })
        .catch(() => {
          if (active) setMessage(t("settings.notifications.permissionUnavailable"));
        });
    };
    refresh();
    const listener = AppState.addEventListener("change", (state) => {
      if (state === "active") refresh();
    });
    return () => {
      active = false;
      listener.remove();
    };
  }, [t]);
  const requestPermission = useCallback(() => {
    setBusy(true);
    setMessage(null);
    void (async () => {
      if (permission && !isPushPermissionGranted(permission) && !permission.canAskAgain) {
        await Linking.openSettings();
      } else {
        await ensureNativeNotificationPermission();
        setPermission(await Notifications.getPermissionsAsync());
        refreshPushRegistrations();
      }
    })()
      .catch(() => setMessage(t("settings.notifications.permissionUnavailable")))
      .finally(() => setBusy(false));
  }, [permission, t]);
  const sendTest = useCallback(() => {
    setBusy(true);
    void sendOsNotification({
      title: t("settings.notifications.test"),
      body: t("settings.notifications.testHint"),
    })
      .then((sent) =>
        setMessage(
          t(sent ? "settings.notifications.sentTitle" : "settings.notifications.sendFailedTitle"),
        ),
      )
      .finally(() => setBusy(false));
  }, [t]);
  const granted = permission !== null && isPushPermissionGranted(permission);
  return (
    <SettingsSection title={t("settings.notifications.title")}>
      <View style={settingsStyles.card}>
        <Text style={settingsStyles.rowHint}>{t("onlineService.accountPushHint")}</Text>
        <View style={settingsStyles.row}>
          <View style={settingsStyles.rowContent}>
            <Text style={settingsStyles.rowTitle}>{t("settings.notifications.permission")}</Text>
            <Text style={settingsStyles.rowHint}>
              {t(
                granted
                  ? "desktop.permissions.notifications.allowed"
                  : "settings.notifications.permissionRequired",
              )}
            </Text>
          </View>
          <Button
            variant="outline"
            size="sm"
            disabled={busy || !permission || granted}
            onPress={requestPermission}
          >
            {t(
              granted
                ? "settings.permissions.actions.granted"
                : permission?.canAskAgain
                  ? "settings.permissions.actions.request"
                  : "settings.notifications.openSettings",
            )}
          </Button>
        </View>
        <View style={[settingsStyles.row, settingsStyles.rowBorder]}>
          <View style={settingsStyles.rowContent}>
            <Text style={settingsStyles.rowTitle}>{t("settings.notifications.test")}</Text>
          </View>
          <Button variant="outline" size="sm" disabled={busy || !granted} onPress={sendTest}>
            {t("settings.notifications.send")}
          </Button>
        </View>
        {message ? (
          <Text accessibilityLiveRegion="polite" style={settingsStyles.rowHint}>
            {message}
          </Text>
        ) : null}
      </View>
    </SettingsSection>
  );
}

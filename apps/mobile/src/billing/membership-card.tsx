import React from "react";
import { Platform, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { SettingsCard, SettingsRow, SettingsSection } from "@/components/settings";
import { Button } from "@/components/ui/button";
import { openExternalUrl } from "@/utils/open-external-url";
import {
  purchaseMembership,
  refreshMembership,
  restoreMembership,
  supportsAppStorePurchases,
  useBillingState,
} from "./state";

const privacyURL = "https://ait-app.com/privacy/";
const termsURL = "https://www.apple.com/legal/internet-services/itunes/dev/stdeula/";
const manageURL = "https://apps.apple.com/account/subscriptions";

export function OnlineServiceMembershipCard() {
  const { t, i18n } = useTranslation();
  const { membership, price, busy, error, message } = useBillingState();
  const native = supportsAppStorePurchases();
  if (!membership && !native && !error) return null;
  const active = membership?.status === "active" || membership?.status === "grace_period";
  const purchasable = native && membership?.purchases_available && membership.required;
  const status = membership
    ? t(`onlineService.billing.states.${membership.status}`, {
        defaultValue: t("onlineService.billing.states.inactive"),
      })
    : t("onlineService.billing.loading");
  const open = (url: string) => {
    void openExternalUrl(url).catch(() =>
      useBillingState.setState({ error: t("onlineService.billing.openFailed") }),
    );
  };
  return (
    <SettingsSection
      title={t("onlineService.billing.title")}
      flush
      testID="online-service-membership"
    >
      <SettingsCard>
        <SettingsRow
          label={t("onlineService.billing.plan")}
          hint={t("onlineService.billing.benefit")}
        >
          <Text style={styles.value}>{status}</Text>
        </SettingsRow>
        {membership?.expires_at ? (
          <SettingsRow label={t("onlineService.billing.accessUntil")}>
            <Text style={styles.value}>
              {new Date(membership.expires_at).toLocaleString(i18n.language)}
            </Text>
          </SettingsRow>
        ) : null}
        {membership && !membership.required ? (
          <Text style={styles.note}>{t("onlineService.billing.accessAvailable")}</Text>
        ) : null}
        <View style={styles.actions}>
          {purchasable && !active ? (
            <Button
              disabled={busy || message === "pending" || !price}
              testID="subscribe-online-service"
              onPress={() => void purchaseMembership()}
            >
              {price
                ? t("onlineService.billing.subscribe", { price })
                : t("onlineService.billing.unavailable")}
            </Button>
          ) : null}
          {native ? (
            <Button
              variant="outline"
              disabled={busy}
              testID="restore-online-service"
              onPress={() => void restoreMembership()}
            >
              {t("onlineService.billing.restore")}
            </Button>
          ) : null}
          <Button variant="ghost" disabled={busy} onPress={() => void refreshMembership()}>
            {t("onlineService.refresh")}
          </Button>
          {Platform.OS === "ios" ? (
            <Button variant="ghost" onPress={() => open(manageURL)}>
              {t("onlineService.billing.manage")}
            </Button>
          ) : null}
        </View>
        {purchasable || active ? (
          <Text style={styles.note}>{t("onlineService.billing.renewal")}</Text>
        ) : null}
        <View style={styles.actions}>
          <Button variant="ghost" size="sm" onPress={() => open(privacyURL)}>
            {t("onlineService.billing.privacy")}
          </Button>
          <Button variant="ghost" size="sm" onPress={() => open(termsURL)}>
            {t("onlineService.billing.terms")}
          </Button>
        </View>
        {message ? (
          <Text accessibilityLiveRegion="polite" style={styles.note}>
            {t(`onlineService.billing.${message}`)}
          </Text>
        ) : null}
        {error ? (
          <Text accessibilityRole="alert" style={styles.error}>
            {error}
          </Text>
        ) : null}
      </SettingsCard>
    </SettingsSection>
  );
}
const styles = StyleSheet.create((theme) => ({
  value: { color: theme.colors.foregroundMuted, fontSize: theme.fontSize.base, flexShrink: 1 },
  note: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    padding: theme.spacing[4],
  },
  error: { color: theme.colors.destructive, padding: theme.spacing[4] },
  actions: {
    flexDirection: "row",
    flexWrap: "wrap",
    gap: theme.spacing[2],
    padding: theme.spacing[3],
  },
}));

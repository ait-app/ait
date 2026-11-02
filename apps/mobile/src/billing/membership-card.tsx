import React from "react";
import { Platform, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { SettingsCard, SettingsRow, SettingsSection } from "@/components/settings";
import { Button } from "@/components/ui/button";
import { subscriptionStore } from "./store";
import { purchaseOption } from "./coordinator";
import { openExternalUrl } from "@/utils/open-external-url";
import {
  purchaseMembership,
  refreshMembership,
  restoreMembership,
  supportsStorePurchases,
  useBillingState,
} from "./state";

const privacyURL = "https://ait-app.com/privacy/";
const appleTermsURL = "https://www.apple.com/legal/internet-services/itunes/dev/stdeula/";
const googleTermsURL = "https://play.google.com/intl/en_us/about/play-terms/";

export function OnlineServiceMembershipCard() {
  const { t, i18n } = useTranslation();
  const { membership, price, busy, error, message } = useBillingState();
  const native = supportsStorePurchases();
  if (!membership && !native && !error) return null;
  const active = membership?.status === "active" || membership?.status === "grace_period";
  const provider =
    subscriptionStore?.provider ?? (Platform.OS === "android" ? "google_play" : "app_store");
  const option = membership ? purchaseOption(membership, { provider }) : null;
  const currentSubscription = membership?.subscriptions?.find((item) => item.store === provider);
  const subscribedHere = currentSubscription
    ? ["active", "grace_period", "pending"].includes(currentSubscription.status) &&
      (currentSubscription.status === "pending" ||
        Date.parse(currentSubscription.expires_at) > Date.now())
    : !membership?.subscriptions && active;
  const purchasable = native && option?.available && membership?.required;
  const termsURL = provider === "google_play" ? googleTermsURL : appleTermsURL;
  const manageURL =
    provider === "google_play"
      ? `https://play.google.com/store/account/subscriptions?sku=${encodeURIComponent(option?.product_id ?? "monthly")}&package=dev.ait.mobile`
      : "https://apps.apple.com/account/subscriptions";
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
        {membership?.subscriptions?.map((item) => (
          <SettingsRow key={item.store} label={t(`onlineService.billing.stores.${item.store}`)}>
            <Text style={styles.value}>
              {t(`onlineService.billing.states.${item.status}`, {
                defaultValue: item.status,
              })}
              {item.status === "active" && !item.auto_renewing
                ? ` · ${t("onlineService.billing.renewalCancelled")}`
                : ""}
            </Text>
          </SettingsRow>
        ))}
        <Text style={styles.note}>{t("onlineService.billing.stacking")}</Text>
        {membership && !membership.required ? (
          <Text style={styles.note}>{t("onlineService.billing.accessAvailable")}</Text>
        ) : null}
        <View style={styles.actions}>
          {purchasable && !subscribedHere ? (
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
          {native ? (
            <Button variant="ghost" onPress={() => open(manageURL)}>
              {t("onlineService.billing.manage")}
            </Button>
          ) : null}
        </View>
        {purchasable || active ? (
          <Text style={styles.note}>{t(`onlineService.billing.renewalByStore.${provider}`)}</Text>
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

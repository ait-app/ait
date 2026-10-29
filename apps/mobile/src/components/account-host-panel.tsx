import { OnlineServiceMembershipCard } from "@/billing/membership-card";
import { supportsBrowserAccount } from "@/runtime/browser-account";
import React, { useRef, useState, type ReactNode } from "react";
import { Keyboard, Pressable, Text, View } from "react-native";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import { ChevronDown, ChevronRight, ExternalLink, RotateCw } from "lucide-react-native";
import { useTranslation } from "react-i18next";
import { Button } from "./ui/button";
import { StatusBadge } from "./ui/status-badge";
import { AdaptiveTextInput } from "./adaptive-text-input";
import { accountCommand, useAccountState } from "@/runtime/account-state";
import { confirmDialog } from "@/utils/confirm-dialog";
import { SettingsCard, SettingsRow, SettingsSection } from "./settings";

export function AccountHostPanel({
  onSignedIn,
  onCancel,
  onRefresh,
  children,
}: {
  onSignedIn?: () => void;
  onCancel?: () => void;
  onRefresh?: () => void;
  children?: ReactNode;
}) {
  const { t } = useTranslation();
  const { theme } = useUnistyles();
  const account = useAccountState();
  const accountIdentity = JSON.stringify([account.center, account.accountId, account.name]);
  const currentAccountIdentity = useRef(accountIdentity);
  currentAccountIdentity.current = accountIdentity;
  // Follow the account snapshot until the user edits the optional override.
  const [centerOverride, setCenterOverride] = useState<string | null>(null);
  const center = centerOverride ?? account.center;
  const [showServiceSettings, setShowServiceSettings] = useState(false);
  const [working, setBusy] = useState(false);
  const busy = working || account.loginPending === true;
  const busyRef = useRef(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const runAccountAction = async (work: () => Promise<unknown>) => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await work();
    } catch (error) {
      setError(error instanceof Error ? error.message : t("onlineService.requestFailed"));
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };
  const ServiceSettingsIcon = showServiceSettings ? ChevronDown : ChevronRight;

  const signIn = async () => {
    const snapshot = await accountCommand("account_login_hosted", { center });
    if (snapshot.status === "logged_out") return;
    Keyboard.dismiss();
    onSignedIn?.();
  };

  return (
    <View style={styles.panel} testID="account-host-panel">
      {account.status === "logged_out" ? (
        <>
          <SettingsSection title={t("onlineService.accountTitle")} flush>
            <SettingsCard>
              <SettingsRow
                label={t("onlineService.signIn")}
                hint={t("onlineService.loginDescription")}
              >
                <View style={styles.actions}>
                  {account.loginPending ? (
                    <Button
                      variant="ghost"
                      size="sm"
                      onPress={() => void accountCommand("account_cancel_login")}
                      testID="account-cancel-login"
                    >
                      {t("common.actions.cancel")}
                    </Button>
                  ) : onCancel ? (
                    <Button variant="ghost" size="sm" onPress={onCancel} disabled={busy}>
                      {t("common.actions.cancel")}
                    </Button>
                  ) : null}
                  <Button
                    variant="default"
                    size="sm"
                    leftIcon={ExternalLink}
                    disabled={busy}
                    loading={busy}
                    testID="account-unified-login"
                    onPress={() => void runAccountAction(signIn)}
                  >
                    {t(
                      account.loginPending
                        ? "onlineService.browserWaiting"
                        : "onlineService.unifiedLogin",
                    )}
                  </Button>
                </View>
              </SettingsRow>
              {!supportsBrowserAccount() ? (
                <Pressable
                  accessibilityRole="button"
                  accessibilityState={{ expanded: showServiceSettings, disabled: busy }}
                  style={styles.disclosure}
                  disabled={busy}
                  onPress={() => setShowServiceSettings((shown) => !shown)}
                  testID="account-service-settings"
                >
                  <Text style={styles.disclosureText}>{t("onlineService.serviceSettings")}</Text>
                  <ServiceSettingsIcon size={16} color={theme.colors.foregroundMuted} />
                </Pressable>
              ) : null}
              {!supportsBrowserAccount() && showServiceSettings ? (
                <SettingsRow label={t("onlineService.serviceUrl")}>
                  <AdaptiveTextInput
                    style={styles.input}
                    initialValue={center}
                    resetKey={account.center}
                    onChangeText={setCenterOverride}
                    placeholder={t("onlineService.serviceUrlHint")}
                    accessibilityLabel={t("onlineService.serviceUrl")}
                    autoCapitalize="none"
                    autoCorrect={false}
                    editable={!busy}
                    testID="account-center"
                  />
                </SettingsRow>
              ) : null}
            </SettingsCard>
          </SettingsSection>
          {error || account.error ? (
            <Text style={styles.error} accessibilityRole="alert">
              {error ?? account.error}
            </Text>
          ) : null}
        </>
      ) : (
        <>
          <View style={styles.accountHeader}>
            <View style={styles.accountSummary}>
              <Text selectable numberOfLines={1} style={styles.title}>
                {account.name}
              </Text>
              <StatusBadge
                label={t(
                  account.status === "online"
                    ? "common.connectionStatus.online"
                    : "common.connectionStatus.connecting",
                )}
                variant={account.status === "online" ? "success" : "muted"}
                leading={
                  <View
                    style={[
                      styles.statusDot,
                      account.status === "online" ? styles.onlineDot : null,
                    ]}
                  />
                }
              />
            </View>
            <Button
              variant="ghost"
              size="sm"
              leftIcon={<RotateCw size={theme.iconSize.md} color={theme.colors.foregroundMuted} />}
              disabled={busy}
              accessibilityLabel={t("onlineService.refresh")}
              onPress={() =>
                void runAccountAction(async () => {
                  await accountCommand("account_refresh");
                  onRefresh?.();
                })
              }
            >
              {t("onlineService.refresh")}
            </Button>
          </View>
          <OnlineServiceMembershipCard />
          {children ?? (
            <SettingsSection title={t("onlineService.accountInfo")} flush>
              <SettingsCard>
                {!supportsBrowserAccount() ? (
                  <SettingsRow label={t("onlineService.serviceUrl")}>
                    <Text selectable style={styles.value}>
                      {account.center}
                    </Text>
                  </SettingsRow>
                ) : null}
                <SettingsRow label={t("onlineService.accountExpiry")}>
                  <Text selectable style={styles.value}>
                    {account.accountExpiresAt
                      ? new Date(account.accountExpiresAt).toLocaleString()
                      : "—"}
                  </Text>
                </SettingsRow>
              </SettingsCard>
            </SettingsSection>
          )}
          <View style={styles.actions}>
            <Button
              variant="destructive"
              size="sm"
              disabled={busy}
              testID="account-delete"
              onPress={() =>
                void runAccountAction(async () => {
                  const confirmed = await confirmDialog({
                    title: t("onlineService.deleteAccountTitle"),
                    message: `${t("onlineService.deleteAccountMessage")}\n\n${t("onlineService.billing.deleteWarning")}`,
                    confirmLabel: t("onlineService.deleteAccount"),
                    cancelLabel: t("common.actions.cancel"),
                    destructive: true,
                  });
                  if (!confirmed || currentAccountIdentity.current !== accountIdentity) return;
                  const snapshot = await accountCommand("account_delete");
                  // The service asked for confirmation in the browser it just opened.
                  if (snapshot.status !== "logged_out")
                    setNotice(t("onlineService.deleteAccountInBrowser"));
                })
              }
            >
              {t("onlineService.deleteAccount")}
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={busy}
              onPress={() => void runAccountAction(() => accountCommand("account_logout"))}
            >
              {t("onlineService.signOut")}
            </Button>
          </View>
          {notice ? (
            <Text style={styles.notice} testID="account-delete-notice">
              {notice}
            </Text>
          ) : null}
          {error || account.error ? (
            <Text style={styles.error} accessibilityRole="alert">
              {error ?? account.error}
            </Text>
          ) : null}
        </>
      )}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  panel: {
    gap: theme.spacing[6],
  },
  title: {
    flexShrink: 1,
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
    color: theme.colors.foreground,
  },
  value: { fontSize: theme.fontSize.base, color: theme.colors.foregroundMuted, flexShrink: 1 },
  input: {
    minWidth: 180,
    backgroundColor: theme.colors.surface2,
    paddingHorizontal: theme.spacing[4],
    paddingVertical: theme.spacing[3],
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: theme.borderRadius.lg,
    color: theme.colors.foreground,
  },
  actions: {
    flexDirection: "row",
    flexWrap: "wrap",
    justifyContent: "flex-end",
    gap: theme.spacing[2],
  },
  disclosure: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
    justifyContent: "space-between",
    padding: theme.spacing[4],
  },
  disclosureText: {
    fontSize: theme.fontSize.base,
    flexShrink: 1,
    color: theme.colors.foreground,
  },
  accountHeader: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: theme.spacing[2],
  },
  accountSummary: {
    flex: 1,
    minWidth: 0,
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
  },
  statusDot: {
    width: 6,
    height: 6,
    borderRadius: 3,
    backgroundColor: theme.colors.foregroundMuted,
  },
  onlineDot: { backgroundColor: theme.colors.statusDotSuccess },
  notice: { color: theme.colors.foregroundMuted, fontSize: theme.fontSize.base },
  error: { color: theme.colors.destructive, fontSize: theme.fontSize.base },
}));

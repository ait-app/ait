import { supportsBrowserAccount } from "@/runtime/browser-account";
import React, { useRef, useState, type ReactNode } from "react";
import { Keyboard, Text, View } from "react-native";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import { ExternalLink, RotateCw } from "lucide-react-native";
import { useTranslation } from "react-i18next";
import { Button } from "./ui/button";
import { StatusBadge } from "./ui/status-badge";
import { AdaptiveTextInput } from "./adaptive-text-input";
import { accountCommand, useAccountState } from "@/runtime/account-state";
import type { AccountEntry } from "@ait/client/internal/account-session";
import { confirmDialog } from "@/utils/confirm-dialog";
import { SettingsCard, SettingsRow, SettingsSection, SettingsSelect } from "./settings";

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
  const browserAccount = supportsBrowserAccount();
  const entry = account.entry ?? { mode: "auto" };
  const [editingCustom, setEditingCustom] = useState(false);
  const [customDraft, setCustomDraft] = useState<string | null>(null);
  const savedCustom = entry.mode === "custom" ? entry.center : "";
  const customCenter = customDraft ?? savedCustom;
  const showCustom = editingCustom || entry.mode === "custom";
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
  const mode = entry.mode;

  const changeEntry = async (entry: AccountEntry) => {
    await accountCommand("account_change_entry", { entry });
    setCustomDraft(null);
    setEditingCustom(false);
  };
  const choice = editingCustom ? "custom" : mode;
  const saveCustom = () => changeEntry({ mode: "custom", center: customCenter });
  const entrySelect = !browserAccount && (
    <SettingsSelect<AccountEntry["mode"]>
      testID="account-service-entry"
      label={t("onlineService.serviceEntry")}
      value={choice}
      options={[
        { value: "auto", label: t("onlineService.entryAuto") },
        { value: "cn", label: t("onlineService.entryChina") },
        { value: "global", label: t("onlineService.entryGlobal") },
        { value: "custom", label: t("onlineService.entryCustom") },
      ]}
      disabled={busy}
      onValueChange={(value) => {
        if (value === "custom") setEditingCustom(true);
        else if (value !== mode || editingCustom)
          void runAccountAction(() => changeEntry({ mode: value }));
      }}
    />
  );
  const customSettings = !browserAccount && showCustom && (
    <SettingsRow
      label={t("onlineService.serviceUrl")}
      hint={account.status !== "logged_out" ? t("onlineService.entryNextLogin") : undefined}
    >
      <View style={styles.actions}>
        <AdaptiveTextInput
          style={styles.input}
          initialValue={customCenter}
          resetKey={savedCustom}
          onChangeText={setCustomDraft}
          placeholder="https://your-server.example/api"
          accessibilityLabel={t("onlineService.serviceUrl")}
          autoCapitalize="none"
          autoCorrect={false}
          editable={!busy}
          testID="account-center"
        />
        <Button
          variant="outline"
          size="sm"
          disabled={busy || !customCenter.trim() || customCenter.trim() === savedCustom}
          onPress={() => void runAccountAction(saveCustom)}
        >
          {t("common.actions.save")}
        </Button>
      </View>
    </SettingsRow>
  );

  const signIn = async () => {
    if (showCustom && (entry.mode !== "custom" || customCenter.trim() !== savedCustom)) {
      await saveCustom();
    }
    const snapshot = await accountCommand("account_login_hosted");
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
                    disabled={busy || (showCustom && !customCenter.trim())}
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
              {entrySelect}
              {customSettings}
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
          {!browserAccount && (
            <SettingsSection title={t("onlineService.serviceEntry")} flush>
              <SettingsCard>
                {entrySelect}
                {customSettings}
                {children ? null : (
                  <SettingsRow label={t("onlineService.activeEntry")}>
                    <Text selectable style={styles.value}>
                      {account.center}
                    </Text>
                  </SettingsRow>
                )}
              </SettingsCard>
            </SettingsSection>
          )}
          {children ?? (
            <SettingsSection title={t("onlineService.accountInfo")} flush>
              <SettingsCard>
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
                    message: t("onlineService.deleteAccountMessage"),
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

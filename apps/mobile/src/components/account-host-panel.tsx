import React, { useEffect, useRef, useState } from "react";
import { Keyboard, Pressable, Text, View } from "react-native";
import { useRouter } from "expo-router";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import {
  Check,
  ChevronDown,
  ChevronRight,
  ExternalLink,
  Monitor,
  RefreshCw,
} from "lucide-react-native";
import { useTranslation } from "react-i18next";
import { Button } from "./ui/button";
import { AdaptiveTextInput } from "./adaptive-text-input";
import {
  accountCommand,
  accountLoginMethods,
  useAccountState,
  type AccountHost,
} from "@/runtime/account-state";

export function AccountHostPanel({
  onConnected,
  onCancel,
  showHosts = true,
}: {
  onConnected?: (serverId: string) => void;
  onCancel?: () => void;
  showHosts?: boolean;
}) {
  const { t } = useTranslation();
  const { theme } = useUnistyles();
  const account = useAccountState();
  const router = useRouter();
  // Follow the account snapshot until the user edits the optional override.
  const [centerOverride, setCenterOverride] = useState<string | null>(null);
  const center = centerOverride ?? account.center;
  const [showServiceSettings, setShowServiceSettings] = useState(false);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [passwordReset, setPasswordReset] = useState(0);
  const [working, setBusy] = useState(false);
  const busy = working || account.loginPending === true;
  const [hosted, setHosted] = useState(false);
  const [legacyLogin, setLegacyLogin] = useState(false);
  useEffect(() => {
    let current = true;
    setHosted(false);
    setLegacyLogin(false);
    if (account.status !== "logged_out") return;
    void accountLoginMethods(center)
      .then((methods) => {
        if (current) setHosted(methods.hosted);
      })
      .catch(() => {
        /* Keep legacy login available when discovery is unreachable. */
      });
    return () => {
      current = false;
    };
  }, [center, account.status]);
  const busyRef = useRef(false);
  const [error, setError] = useState<string | null>(null);
  const runAccountAction = async (work: () => Promise<unknown>) => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    try {
      await work();
    } catch (error) {
      setError(error instanceof Error ? error.message : t("onlineService.requestFailed"));
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };
  const loginDisabled = busy || !email.trim() || !password;
  const useHostedLogin = hosted && !legacyLogin;
  const ServiceSettingsIcon = showServiceSettings ? ChevronDown : ChevronRight;

  const login = () => {
    if (loginDisabled) return;
    const secret = password;
    Keyboard.dismiss();
    setPassword("");
    setPasswordReset((reset) => reset + 1);
    void runAccountAction(() =>
      accountCommand("account_login", { center, email, password: secret }),
    );
  };
  const selectHost = (host: AccountHost) => {
    void runAccountAction(async () => {
      await accountCommand("account_select", { hostId: host.host_id });
      Keyboard.dismiss();
      // A sheet owns dismissal and navigation. Its native subtree must be removed
      // before the navigator starts mounting the selected host's screen.
      if (onConnected) onConnected(host.server_id);
      else router.push(`/h/${host.server_id}`);
    });
  };

  return (
    <View style={styles.panel} testID="account-host-panel">
      {account.status === "logged_out" ? (
        <>
          <Text style={styles.hint}>{t("onlineService.loginDescription")}</Text>
          {hosted ? (
            <>
              {useHostedLogin ? (
                <Text style={styles.hint}>{t("onlineService.unifiedHint")}</Text>
              ) : null}
              <Pressable
                accessibilityRole="button"
                disabled={busy}
                style={styles.disclosure}
                onPress={() => setLegacyLogin((shown) => !shown)}
                testID="account-legacy-login"
              >
                <ChevronRight size={16} color={theme.colors.foregroundMuted} />
                <Text style={styles.disclosureText}>
                  {t(legacyLogin ? "onlineService.unifiedLogin" : "onlineService.legacyLogin")}
                </Text>
              </Pressable>
            </>
          ) : null}
          {!hosted || legacyLogin ? (
            <>
              <View style={styles.field}>
                <Text style={styles.label}>{t("onlineService.email")}</Text>
                <AdaptiveTextInput
                  style={styles.input}
                  initialValue={email}
                  onChangeText={setEmail}
                  placeholder={t("onlineService.email")}
                  accessibilityLabel={t("onlineService.email")}
                  inputMode="email"
                  keyboardType="email-address"
                  autoComplete="email"
                  textContentType="emailAddress"
                  maxLength={320}
                  autoCapitalize="none"
                  autoCorrect={false}
                  editable={!busy}
                  testID="account-email"
                />
              </View>
              <View style={styles.field}>
                <Text style={styles.label}>{t("onlineService.password")}</Text>
                <AdaptiveTextInput
                  style={styles.input}
                  initialValue={password}
                  resetKey={passwordReset}
                  onChangeText={setPassword}
                  placeholder={t("onlineService.password")}
                  accessibilityLabel={t("onlineService.password")}
                  autoCapitalize="none"
                  autoCorrect={false}
                  secureTextEntry
                  autoComplete="current-password"
                  textContentType="password"
                  maxLength={512}
                  editable={!busy}
                  onSubmitEditing={login}
                  testID="account-password"
                />
              </View>
            </>
          ) : null}
          <View style={styles.field}>
            <Pressable
              accessibilityRole="button"
              accessibilityState={{ expanded: showServiceSettings }}
              style={styles.disclosure}
              disabled={busy}
              onPress={() => setShowServiceSettings((shown) => !shown)}
              testID="account-service-settings"
            >
              <ServiceSettingsIcon size={16} color={theme.colors.foregroundMuted} />
              <Text style={styles.disclosureText}>{t("onlineService.serviceSettings")}</Text>
            </Pressable>
            {showServiceSettings ? (
              <View style={styles.field}>
                <Text style={styles.label}>{t("onlineService.serviceUrl")}</Text>
                <AdaptiveTextInput
                  style={styles.input}
                  initialValue={center}
                  resetKey={account.center}
                  onChangeText={setCenterOverride}
                  placeholder="https://your-server.example/api"
                  accessibilityLabel={t("onlineService.serviceUrl")}
                  autoCapitalize="none"
                  autoCorrect={false}
                  editable={!busy}
                  testID="account-center"
                />
                <Text style={styles.hint}>{t("onlineService.serviceUrlHint")}</Text>
              </View>
            ) : null}
          </View>
          {error || account.error ? (
            <Text style={styles.error} accessibilityRole="alert">
              {error ?? account.error}
            </Text>
          ) : null}
          <View style={styles.actions}>
            {account.loginPending ? (
              <Button
                style={styles.action}
                onPress={() => void accountCommand("account_cancel_login")}
                testID="account-cancel-login"
              >
                {t("common.actions.cancel")}
              </Button>
            ) : onCancel ? (
              <Button style={styles.action} onPress={onCancel} disabled={busy}>
                {t("common.actions.cancel")}
              </Button>
            ) : null}
            {useHostedLogin ? (
              <Button
                style={styles.action}
                variant="default"
                leftIcon={ExternalLink}
                disabled={busy}
                loading={busy}
                testID="account-unified-login"
                onPress={() =>
                  void runAccountAction(() => accountCommand("account_login_hosted", { center }))
                }
              >
                {t(
                  account.loginPending
                    ? "onlineService.browserWaiting"
                    : "onlineService.unifiedLogin",
                )}
              </Button>
            ) : (
              <Button
                style={styles.action}
                variant="default"
                loading={busy}
                disabled={loginDisabled}
                onPress={login}
                testID="account-login"
              >
                {t(busy ? "onlineService.signingIn" : "onlineService.signIn")}
              </Button>
            )}
          </View>
        </>
      ) : (
        <>
          <View style={styles.accountSummary}>
            <View style={styles.hostBody}>
              <Text style={styles.title}>{account.name}</Text>
              <View style={styles.connectionStatus}>
                <View
                  style={[styles.statusDot, account.status === "online" ? styles.onlineDot : null]}
                />
                <Text style={styles.hint}>
                  {t(
                    account.status === "online"
                      ? "onlineService.connected"
                      : "onlineService.connecting",
                  )}
                </Text>
              </View>
            </View>
            <Button
              variant="ghost"
              size="sm"
              disabled={busy}
              onPress={() => void runAccountAction(() => accountCommand("account_logout"))}
            >
              {t("onlineService.signOut")}
            </Button>
          </View>
          <Text style={styles.hint}>{account.center}</Text>
          {account.accountExpiresAt ? (
            <Text style={styles.hint}>
              {t("onlineService.expiresAt", {
                date: new Date(account.accountExpiresAt).toLocaleString(),
              })}
            </Text>
          ) : null}
          {showHosts ? (
            <>
              <View style={styles.hostHeading}>
                <View style={styles.field}>
                  <Text style={styles.label}>{t("onlineService.onlineHosts")}</Text>
                  <Text style={styles.hint}>
                    {account.stale
                      ? t("onlineService.staleHosts")
                      : t("onlineService.hostCount", { count: account.hosts.length })}
                  </Text>
                </View>
                <Button
                  variant="ghost"
                  size="sm"
                  leftIcon={RefreshCw}
                  disabled={busy}
                  onPress={() => void runAccountAction(() => accountCommand("account_refresh"))}
                >
                  {t("onlineService.refresh")}
                </Button>
              </View>
              {account.hosts.map((host) => (
                <Pressable
                  key={host.host_id}
                  disabled={busy}
                  style={[
                    styles.host,
                    account.selected?.host_id === host.host_id ? styles.selectedHost : null,
                    busy ? styles.disabled : null,
                  ]}
                  accessibilityRole="button"
                  accessibilityState={{
                    disabled: busy,
                    selected: account.selected?.host_id === host.host_id,
                  }}
                  testID={`account-host-${host.host_id}`}
                  onPress={() => selectHost(host)}
                >
                  <View style={styles.hostIcon}>
                    <Monitor size={18} color={theme.colors.foregroundMuted} />
                  </View>
                  <View style={styles.hostBody}>
                    <Text style={styles.title} numberOfLines={1}>
                      {host.name}
                      {account.selected?.host_id === host.host_id
                        ? ` · ${t("onlineService.selected")}`
                        : ""}
                    </Text>
                    <Text style={styles.hint}>{host.platform}</Text>
                  </View>
                  {account.selected?.host_id === host.host_id ? (
                    <Check size={18} color={theme.colors.accent} />
                  ) : (
                    <ChevronRight size={18} color={theme.colors.foregroundMuted} />
                  )}
                </Pressable>
              ))}
              {!account.stale && !account.hosts.length ? (
                <View style={styles.emptyHosts}>
                  <Monitor size={24} color={theme.colors.foregroundMuted} />
                  <Text style={styles.emptyHint}>{t("onlineService.emptyHosts")}</Text>
                </View>
              ) : null}
              {account.selected ? (
                <Button
                  variant="outline"
                  disabled={busy}
                  onPress={() =>
                    void runAccountAction(() => accountCommand("account_select", { hostId: null }))
                  }
                >
                  {t("onlineService.disconnectRemoteHost")}
                </Button>
              ) : null}
            </>
          ) : (
            <>
              <Button
                variant="outline"
                leftIcon={RefreshCw}
                disabled={busy}
                onPress={() => void runAccountAction(() => accountCommand("account_refresh"))}
              >
                {t("onlineService.refresh")}
              </Button>
              <Text style={styles.hint}>{t("onlineService.hostSettingsHint")}</Text>
              <Text style={styles.hint}>{t("onlineService.signOutHint")}</Text>
            </>
          )}
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
    gap: theme.spacing[4],
  },
  field: { gap: theme.spacing[2] },
  label: {
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
    color: theme.colors.foregroundMuted,
  },
  title: {
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
    color: theme.colors.foreground,
  },
  hint: { fontSize: theme.fontSize.base, lineHeight: 22, color: theme.colors.foregroundMuted },
  input: {
    backgroundColor: theme.colors.surface2,
    paddingHorizontal: theme.spacing[4],
    paddingVertical: theme.spacing[3],
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: theme.borderRadius.lg,
    color: theme.colors.foreground,
  },
  actions: { flexDirection: "row", gap: theme.spacing[3], marginTop: theme.spacing[2] },
  action: { flex: 1 },
  disclosure: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
    alignSelf: "flex-start",
    paddingVertical: theme.spacing[1],
  },
  disclosureText: {
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
    color: theme.colors.foreground,
  },
  accountSummary: { flexDirection: "row", alignItems: "center", gap: theme.spacing[3] },
  connectionStatus: { flexDirection: "row", alignItems: "center", gap: theme.spacing[2] },
  statusDot: {
    width: 6,
    height: 6,
    borderRadius: 3,
    backgroundColor: theme.colors.foregroundMuted,
  },
  onlineDot: { backgroundColor: theme.colors.accent },
  hostHeading: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: theme.spacing[3],
    marginTop: theme.spacing[2],
  },
  host: {
    flexDirection: "row",
    alignItems: "center",
    padding: theme.spacing[4],
    gap: theme.spacing[3],
    backgroundColor: theme.colors.surface2,
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: theme.borderRadius.lg,
  },
  hostIcon: { width: 32, alignItems: "center", justifyContent: "center" },
  hostBody: { flex: 1, minWidth: 0, gap: theme.spacing[1] },
  selectedHost: { borderColor: theme.colors.accent },
  disabled: { opacity: theme.opacity[50] },
  emptyHosts: {
    alignItems: "center",
    gap: theme.spacing[3],
    padding: theme.spacing[6],
    backgroundColor: theme.colors.surface2,
    borderRadius: theme.borderRadius.lg,
    borderWidth: 1,
    borderColor: theme.colors.border,
  },
  emptyHint: {
    textAlign: "center",
    fontSize: theme.fontSize.base,
    lineHeight: 22,
    color: theme.colors.foregroundMuted,
  },
  error: { color: theme.colors.destructive, fontSize: theme.fontSize.base },
}));

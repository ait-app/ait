import React, { useRef, useState } from "react";
import { Keyboard, Pressable, Text, TextInput, View } from "react-native";
import { useRouter } from "expo-router";
import { StyleSheet } from "react-native-unistyles";
import { useTranslation } from "react-i18next";
import { Button } from "./ui/button";
import { accountCommand, useAccountState, type AccountHost } from "@/runtime/account-state";

export function AccountHostPanel({
  onConnected,
  showHosts = true,
}: {
  onConnected?: (serverId: string) => void;
  showHosts?: boolean;
}) {
  const { t } = useTranslation();
  const account = useAccountState();
  const router = useRouter();
  // Follow the account snapshot until the user edits the optional override.
  const [centerOverride, setCenterOverride] = useState<string | null>(null);
  const center = centerOverride ?? account.center;
  const [showServiceSettings, setShowServiceSettings] = useState(false);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
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

  const login = () => {
    if (loginDisabled) return;
    const secret = password;
    Keyboard.dismiss();
    setPassword("");
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
          <TextInput
            style={styles.input}
            value={email}
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
          <TextInput
            style={styles.input}
            value={password}
            onChangeText={setPassword}
            placeholder={t("onlineService.password")}
            accessibilityLabel={t("onlineService.password")}
            secureTextEntry
            autoComplete="current-password"
            textContentType="password"
            maxLength={512}
            editable={!busy}
            onSubmitEditing={login}
            testID="account-password"
          />
          <Button disabled={loginDisabled} onPress={login} testID="account-login">
            {t(busy ? "onlineService.signingIn" : "onlineService.signIn")}
          </Button>
          <Pressable
            accessibilityRole="button"
            accessibilityState={{ expanded: showServiceSettings }}
            onPress={() => setShowServiceSettings((shown) => !shown)}
            testID="account-service-settings"
          >
            <Text style={styles.hint}>
              {t(
                showServiceSettings
                  ? "onlineService.hideServiceSettings"
                  : "onlineService.serviceSettings",
              )}
            </Text>
          </Pressable>
          {showServiceSettings ? (
            <>
              <Text style={styles.hint}>{t("onlineService.serviceUrlHint")}</Text>
              <TextInput
                style={styles.input}
                value={center}
                onChangeText={setCenterOverride}
                placeholder="https://your-server.example/api"
                accessibilityLabel={t("onlineService.serviceUrl")}
                autoCapitalize="none"
                autoCorrect={false}
                editable={!busy}
                testID="account-center"
              />
            </>
          ) : null}
        </>
      ) : (
        <>
          <Text style={styles.hint}>
            {account.name} ·{" "}
            {t(
              account.status === "online" ? "onlineService.connected" : "onlineService.connecting",
            )}
          </Text>
          <Text style={styles.hint}>{account.center}</Text>
          <View style={styles.actions}>
            <Button
              disabled={busy}
              onPress={() => void runAccountAction(() => accountCommand("account_refresh"))}
            >
              {t("onlineService.refresh")}
            </Button>
            <Button
              disabled={busy}
              onPress={() => void runAccountAction(() => accountCommand("account_logout"))}
            >
              {t("onlineService.signOut")}
            </Button>
          </View>
          <Text style={styles.hint}>{t("onlineService.signOutHint")}</Text>
          {showHosts ? (
            <>
              <Text style={styles.title}>{t("onlineService.onlineHosts")}</Text>
              <Text style={styles.hint}>
                {account.stale
                  ? t("onlineService.staleHosts")
                  : t("onlineService.hostCount", { count: account.hosts.length })}
              </Text>
              {account.hosts.map((host) => (
                <Pressable
                  key={host.host_id}
                  disabled={busy}
                  style={styles.host}
                  accessibilityRole="button"
                  testID={`account-host-${host.host_id}`}
                  onPress={() => selectHost(host)}
                >
                  <Text style={styles.title}>
                    {host.name}
                    {account.selected?.host_id === host.host_id
                      ? ` · ${t("onlineService.selected")}`
                      : ""}
                  </Text>
                  <Text style={styles.hint}>{host.platform}</Text>
                </Pressable>
              ))}
              {!account.stale && !account.hosts.length ? (
                <Text style={styles.hint}>{t("onlineService.emptyHosts")}</Text>
              ) : null}
              {account.selected ? (
                <Button
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
            <Text style={styles.hint}>{t("onlineService.hostSettingsHint")}</Text>
          )}
        </>
      )}
      {error || account.error ? <Text style={styles.error}>{error ?? account.error}</Text> : null}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  panel: {
    gap: theme.spacing[3],
    padding: theme.spacing[4],
    borderRadius: theme.borderRadius.xl,
    backgroundColor: theme.colors.surface2,
  },
  title: { fontSize: theme.fontSize.base, color: theme.colors.foreground },
  hint: { fontSize: theme.fontSize.sm, color: theme.colors.foregroundMuted },
  input: {
    padding: theme.spacing[3],
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: theme.borderRadius.lg,
    color: theme.colors.foreground,
  },
  actions: { flexDirection: "row", gap: theme.spacing[3] },
  host: {
    padding: theme.spacing[3],
    gap: theme.spacing[1],
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: theme.borderRadius.lg,
  },
  error: { color: theme.colors.destructive, fontSize: theme.fontSize.sm },
}));

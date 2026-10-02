import React, { useState } from "react";
import { Pressable, Text, TextInput, View } from "react-native";
import { useRouter } from "expo-router";
import { StyleSheet } from "react-native-unistyles";
import { Button } from "./ui/button";
import { accountCommand, useAccountState, type AccountHost } from "@/runtime/account-state";

export function AccountHostPanel({ onConnected }: { onConnected?: () => void }) {
  const account = useAccountState();
  const router = useRouter();
  // Follow the main process snapshot until the user edits the optional override.
  const [centerOverride, setCenterOverride] = useState<string | null>(null);
  const center = centerOverride ?? account.center;
  const [showServiceSettings, setShowServiceSettings] = useState(false);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const runAccountAction = async (work: () => Promise<unknown>) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await work();
    } catch (error) {
      setError(error instanceof Error ? error.message : "Request failed.");
    } finally {
      setBusy(false);
    }
  };
  const loginDisabled = busy || !email.trim() || !password;

  const login = () => {
    if (loginDisabled) return;
    const secret = password;
    setPassword("");
    void runAccountAction(() =>
      accountCommand("account_login", { center, email, password: secret }),
    );
  };
  const selectHost = (host: AccountHost) => {
    void runAccountAction(async () => {
      await accountCommand("account_select", { hostId: host.host_id });
      onConnected?.();
      router.push(`/h/${host.server_id}`);
    });
  };

  return (
    <View style={styles.panel} testID="account-host-panel">
      <Text style={styles.title}>Account and online hosts</Text>
      {account.status === "logged_out" ? (
        <>
          <Text style={styles.hint}>
            Sign in to bring this host online, then select another host on your account to continue
            working.
          </Text>
          <TextInput
            style={styles.input}
            value={email}
            onChangeText={setEmail}
            placeholder="Email"
            accessibilityLabel="Email"
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
            placeholder="Password"
            accessibilityLabel="Password"
            secureTextEntry
            editable={!busy}
            onSubmitEditing={login}
            testID="account-password"
          />
          <Button disabled={loginDisabled} onPress={login} testID="account-login">
            {busy ? "Signing in..." : "Sign in"}
          </Button>
          <Pressable
            accessibilityRole="button"
            accessibilityState={{ expanded: showServiceSettings }}
            onPress={() => setShowServiceSettings((shown) => !shown)}
            testID="account-service-settings"
          >
            <Text style={styles.hint}>
              {showServiceSettings ? "Hide service settings" : "Service settings"}
            </Text>
          </Pressable>
          {showServiceSettings ? (
            <>
              <Text style={styles.hint}>Service URL (leave blank to use the default)</Text>
              <TextInput
                style={styles.input}
                value={center}
                onChangeText={setCenterOverride}
                placeholder="https://your-server.example/api"
                accessibilityLabel="Service URL"
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
            {account.hostOnline ? "This host is online" : "Waiting for this host to come online"}
          </Text>
          <View style={styles.actions}>
            <Button
              disabled={busy}
              onPress={() => void runAccountAction(() => accountCommand("account_refresh"))}
            >
              Refresh
            </Button>
            <Button
              disabled={busy}
              onPress={() => void runAccountAction(() => accountCommand("account_logout"))}
            >
              Sign out
            </Button>
          </View>
          <Text style={styles.hint}>
            {account.stale
              ? "Host list is out of date"
              : `${account.hosts.length} other ${account.hosts.length === 1 ? "host" : "hosts"} online`}
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
                {account.selected?.host_id === host.host_id ? " · Selected" : ""}
              </Text>
              <Text style={styles.hint}>{host.platform}</Text>
            </Pressable>
          ))}
          {!account.stale && !account.hosts.length ? (
            <Text style={styles.hint}>
              Open Ait on another machine and sign in with the same account. It will appear here
              automatically.
            </Text>
          ) : null}
          {account.selected ? (
            <Button
              disabled={busy}
              onPress={() =>
                void runAccountAction(() => accountCommand("account_select", { hostId: null }))
              }
            >
              Disconnect remote host
            </Button>
          ) : null}
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

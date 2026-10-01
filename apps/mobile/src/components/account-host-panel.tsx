import { useState } from "react";
import { Pressable, Text, TextInput, View } from "react-native";
import { useRouter } from "expo-router";
import { StyleSheet } from "react-native-unistyles";
import { Button } from "./ui/button";
import { accountCommand, useAccountState } from "@/runtime/account-state";

export function AccountHostPanel({ onConnected }: { onConnected?: () => void }) {
  const account = useAccountState();
  const router = useRouter();
  // Follow the main process snapshot until the user edits the optional override.
  const [centerOverride, setCenterOverride] = useState<string | null>(null);
  const center = centerOverride ?? account.center;
  const [showServiceSettings, setShowServiceSettings] = useState(false);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async (work: () => Promise<unknown>) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await work();
    } catch (error) {
      setError(error instanceof Error ? error.message : "请求失败");
    } finally {
      setBusy(false);
    }
  };
  const login = () => {
    if (busy || !username.trim() || !password) return;
    const secret = password;
    setPassword("");
    void run(() => accountCommand("account_login", { center, username, password: secret }));
  };
  return (
    <View style={styles.panel} testID="account-host-panel">
      <Text style={styles.title}>账号与在线 Host</Text>
      {account.status === "logged_out" ? (
        <>
          <Text style={styles.hint}>
            登录后，本机自动上线。选择同账号的在线 Host 即可继续工作。
          </Text>
          <TextInput
            style={styles.input}
            value={username}
            onChangeText={setUsername}
            placeholder="用户名"
            accessibilityLabel="用户名"
            autoCapitalize="none"
            autoCorrect={false}
            editable={!busy}
            testID="account-username"
          />
          <TextInput
            style={styles.input}
            value={password}
            onChangeText={setPassword}
            placeholder="密码"
            accessibilityLabel="密码"
            secureTextEntry
            editable={!busy}
            onSubmitEditing={login}
            testID="account-password"
          />
          <Button
            disabled={busy || !username.trim() || !password}
            onPress={login}
            testID="account-login"
          >
            {busy ? "正在登录…" : "登录"}
          </Button>
          <Pressable
            accessibilityRole="button"
            accessibilityState={{ expanded: showServiceSettings }}
            onPress={() => setShowServiceSettings((shown) => !shown)}
            testID="account-service-settings"
          >
            <Text style={styles.hint}>{showServiceSettings ? "收起服务设置" : "服务设置"}</Text>
          </Pressable>
          {showServiceSettings ? (
            <>
              <Text style={styles.hint}>服务地址（留空使用默认服务）</Text>
              <TextInput
                style={styles.input}
                value={center}
                onChangeText={setCenterOverride}
                placeholder="https://your-server.example/api"
                accessibilityLabel="服务地址"
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
            {account.name} · {account.hostOnline ? "本机已上线" : "本机等待上线"}
          </Text>
          <View style={styles.actions}>
            <Button
              disabled={busy}
              onPress={() => void run(() => accountCommand("account_refresh"))}
            >
              刷新
            </Button>
            <Button
              disabled={busy}
              onPress={() => void run(() => accountCommand("account_logout"))}
            >
              退出登录
            </Button>
          </View>
          <Text style={styles.hint}>
            {account.stale ? "列表待更新" : `${account.hosts.length} 台其他 Host 在线`}
          </Text>
          {account.hosts.map((host) => (
            <Pressable
              key={host.host_id}
              disabled={busy}
              style={styles.host}
              accessibilityRole="button"
              testID={`account-host-${host.host_id}`}
              onPress={() =>
                void run(async () => {
                  await accountCommand("account_select", { hostId: host.host_id });
                  onConnected?.();
                  router.push(`/h/${host.server_id}`);
                })
              }
            >
              <Text style={styles.title}>
                {host.name}
                {account.selected?.host_id === host.host_id ? " · 已选择" : ""}
              </Text>
              <Text style={styles.hint}>{host.platform}</Text>
            </Pressable>
          ))}
          {!account.stale && !account.hosts.length ? (
            <Text style={styles.hint}>
              在另一台机器打开 AIT 并登录同一账号，它会自动出现在这里。
            </Text>
          ) : null}
          {account.selected ? (
            <Button
              disabled={busy}
              onPress={() => void run(() => accountCommand("account_select", { hostId: null }))}
            >
              断开远程 Host
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

import { AitLogo } from "@/components/icons/ait-logo";
import { Button } from "@/components/ui/button";
import { isNative } from "@/constants/platform";
import { isElectronRuntime } from "@/desktop/host";
import { formatVersionWithPrefix } from "@/desktop/updates/desktop-updates";
import { getHostRuntimeStore, isHostRuntimeConnected, useHosts } from "@/runtime/host-runtime";
import { supportsAccountRelay } from "@/runtime/account-state";
import type { HostProfile } from "@/types/host-connection";
import { resolveAppVersion } from "@/utils/app-version";
import { buildOpenProjectRoute } from "@/utils/host-routes";
import { openExternalUrl } from "@/utils/open-external-url";
import { useRouter } from "expo-router";
import { Globe, ExternalLink, Link2, QrCode, Settings, Terminal } from "lucide-react-native";
import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { useTranslation } from "react-i18next";
import { Pressable, ScrollView, Text, View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import { AddHostModal } from "./add-host-modal";
import { AddRemoteSshHostModal } from "./add-remote-ssh-host-modal";
import { AccountHostPanel } from "./account-host-panel";
import { AdaptiveModalSheet, type SheetHeader } from "./adaptive-modal-sheet";

interface WelcomeAction {
  key: "account-relay" | "direct-connection" | "remote-ssh";
  label: string;
  testID: string;
  primary: boolean;
  icon: typeof QrCode;
  onPress: () => void;
}

const styles = StyleSheet.create((theme) => ({
  root: {
    flex: 1,
    backgroundColor: theme.colors.surface0,
  },
  scrollView: {
    flex: 1,
  },
  container: {
    flexGrow: 1,
    padding: theme.spacing[6],
    paddingBottom: 0,
    alignItems: "center",
  },
  content: {
    width: "100%",
    flexGrow: 1,
    justifyContent: "center",
    alignItems: "center",
  },
  title: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
    textAlign: "center",
  },
  subtitle: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.base,
    textAlign: "center",
  },
  copyBlock: {
    alignItems: "center",
    gap: theme.spacing[2],
    marginBottom: theme.spacing[12],
  },
  actions: {
    width: "100%",
    maxWidth: 420,
    gap: theme.spacing[3],
  },
  actionButton: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "center",
    gap: theme.spacing[3],
    paddingVertical: theme.spacing[4],
    borderRadius: theme.borderRadius.xl,
    backgroundColor: theme.colors.surface2,
    borderWidth: 1,
    borderColor: theme.colors.border,
  },
  actionButtonPrimary: {
    backgroundColor: theme.colors.accent,
    borderColor: theme.colors.accent,
  },
  actionText: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
  },
  actionTextPrimary: {
    color: theme.colors.accentForeground,
  },
  setupLink: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "center",
    gap: 6,
  },
  setupLinkText: {
    color: theme.colors.accent,
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
  },
  versionLabel: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    textAlign: "center",
    marginTop: theme.spacing[6],
  },
  settingsButton: {
    alignSelf: "center",
    marginTop: theme.spacing[6],
  },
}));

function useAnyHostOnline(serverIds: string[]): string | null {
  const runtime = getHostRuntimeStore();
  return useSyncExternalStore(
    (onStoreChange) => runtime.subscribeAll(onStoreChange),
    () => {
      let firstOnlineServerId: string | null = null;
      let firstOnlineAt: string | null = null;
      for (const serverId of serverIds) {
        const snapshot = runtime.getSnapshot(serverId);
        const lastOnlineAt = snapshot?.lastOnlineAt ?? null;
        if (!isHostRuntimeConnected(snapshot) || !lastOnlineAt) {
          continue;
        }
        if (!firstOnlineAt || lastOnlineAt < firstOnlineAt) {
          firstOnlineAt = lastOnlineAt;
          firstOnlineServerId = serverId;
        }
      }
      return firstOnlineServerId;
    },
    () => {
      let firstOnlineServerId: string | null = null;
      let firstOnlineAt: string | null = null;
      for (const serverId of serverIds) {
        const snapshot = runtime.getSnapshot(serverId);
        const lastOnlineAt = snapshot?.lastOnlineAt ?? null;
        if (!isHostRuntimeConnected(snapshot) || !lastOnlineAt) {
          continue;
        }
        if (!firstOnlineAt || lastOnlineAt < firstOnlineAt) {
          firstOnlineAt = lastOnlineAt;
          firstOnlineServerId = serverId;
        }
      }
      return firstOnlineServerId;
    },
  );
}

export interface WelcomeScreenProps {
  onHostAdded?: (profile: HostProfile) => void;
}

export function WelcomeScreen({ onHostAdded }: WelcomeScreenProps) {
  const { theme } = useUnistyles();
  const { t } = useTranslation();
  const insets = useSafeAreaInsets();
  const router = useRouter();
  const appVersion = resolveAppVersion();
  const appVersionText = formatVersionWithPrefix(appVersion);
  const [isDirectOpen, setIsDirectOpen] = useState(false);
  const [isRemoteSshOpen, setIsRemoteSshOpen] = useState(false);
  const [isAccountOpen, setIsAccountOpen] = useState(false);
  const [isAccountPresented, setIsAccountPresented] = useState(false);
  const pendingAccountServerId = useRef<string | null>(null);
  const hasNavigated = useRef(false);
  const accountAvailable = supportsAccountRelay();
  const accountHeader = useMemo<SheetHeader>(() => ({ title: t("onlineService.title") }), [t]);
  const hosts = useHosts();
  const anyOnlineServerId = useAnyHostOnline(hosts.map((h) => h.serverId));

  useEffect(() => {
    if (!anyOnlineServerId || isAccountPresented || hasNavigated.current) return;
    hasNavigated.current = true;
    router.replace(buildOpenProjectRoute());
  }, [anyOnlineServerId, isAccountPresented, router]);

  const finishOnboarding = useCallback(() => {
    if (hasNavigated.current) return;
    hasNavigated.current = true;
    router.replace(buildOpenProjectRoute());
  }, [router]);

  const handleOpenAitSite = useCallback(() => {
    void openExternalUrl("https://github.com/ait-app/ait");
  }, []);

  const handleOpenSettings = useCallback(() => {
    router.push("/settings");
  }, [router]);

  const handleOpenDirect = useCallback(() => setIsDirectOpen(true), []);
  const handleCloseDirect = useCallback(() => setIsDirectOpen(false), []);
  const handleOpenRemoteSsh = useCallback(() => setIsRemoteSshOpen(true), []);
  const handleCloseRemoteSsh = useCallback(() => setIsRemoteSshOpen(false), []);
  const handleOpenAccount = useCallback(() => {
    setIsAccountPresented(true);
    setIsAccountOpen(true);
  }, []);
  const handleCloseAccount = useCallback(() => setIsAccountOpen(false), []);
  const handleAccountConnected = useCallback((serverId: string) => {
    pendingAccountServerId.current = serverId;
    setIsAccountOpen(false);
  }, []);
  const handleAccountDismissed = useCallback(() => {
    setIsAccountPresented(false);
    const serverId = pendingAccountServerId.current;
    pendingAccountServerId.current = null;
    if (!serverId || hasNavigated.current) return;
    hasNavigated.current = true;
    router.replace(`/h/${serverId}`);
  }, [router]);

  const handleHostSaved = useCallback(
    ({ profile }: { profile: HostProfile; serverId: string }) => {
      onHostAdded?.(profile);
      finishOnboarding();
    },
    [onHostAdded, finishOnboarding],
  );

  const actions: WelcomeAction[] = [
    {
      key: "direct-connection",
      label: t("pairing.connectionMethods.direct.title"),
      testID: "welcome-direct-connection",
      primary: !accountAvailable,
      icon: Link2,
      onPress: handleOpenDirect,
    },
  ];

  if (isElectronRuntime()) {
    actions.splice(1, 0, {
      key: "remote-ssh",
      label: t("pairing.connectionMethods.remoteSsh.title"),
      testID: "welcome-remote-ssh",
      primary: false,
      icon: Terminal,
      onPress: handleOpenRemoteSsh,
    });
  }

  if (accountAvailable) {
    actions.splice(1, 0, {
      key: "account-relay",
      label: t("onlineService.title"),
      testID: "welcome-account-relay",
      primary: true,
      icon: Globe,
      onPress: handleOpenAccount,
    });
  }

  const scrollContentContainerStyle = useMemo(
    () => [styles.container, { paddingBottom: theme.spacing[6] + insets.bottom }],
    [theme.spacing, insets.bottom],
  );

  return (
    <View style={styles.root}>
      <ScrollView
        style={styles.scrollView}
        contentContainerStyle={scrollContentContainerStyle}
        showsVerticalScrollIndicator={false}
        testID="welcome-screen"
      >
        <View style={styles.content}>
          <AitLogo size={160} variant="stacked" wordmarkColor={theme.colors.foreground} />
          <View style={styles.copyBlock}>
            <Text style={styles.title}>{t("onboarding.title")}</Text>
            <Text style={styles.subtitle}>{t("onboarding.subtitle")}</Text>
            {isNative ? (
              <Pressable style={styles.setupLink} onPress={handleOpenAitSite}>
                <Text style={styles.setupLinkText}>Ait on GitHub</Text>
                <ExternalLink size={14} color={theme.colors.accent} />
              </Pressable>
            ) : null}
          </View>

          <View style={styles.actions}>
            {actions.map((action) => (
              <WelcomeActionButton key={action.key} action={action} />
            ))}
          </View>

          <Button
            variant="ghost"
            size="sm"
            leftIcon={Settings}
            onPress={handleOpenSettings}
            style={styles.settingsButton}
            testID="welcome-open-settings"
          >
            {t("onboarding.actions.settings")}
          </Button>
        </View>
        <Text style={styles.versionLabel}>{appVersionText}</Text>

        <AddHostModal
          visible={isDirectOpen}
          onClose={handleCloseDirect}
          onSaved={handleHostSaved}
        />

        <AddRemoteSshHostModal
          visible={isRemoteSshOpen}
          onClose={handleCloseRemoteSsh}
          onSaved={handleHostSaved}
        />
      </ScrollView>
      {accountAvailable ? (
        <AdaptiveModalSheet
          header={accountHeader}
          visible={isAccountOpen}
          onClose={handleCloseAccount}
          onDismiss={handleAccountDismissed}
          testID="welcome-account-sheet"
        >
          <AccountHostPanel onConnected={handleAccountConnected} onCancel={handleCloseAccount} />
        </AdaptiveModalSheet>
      ) : null}
    </View>
  );
}

interface WelcomeActionButtonProps {
  action: WelcomeAction;
}

function WelcomeActionButton({ action }: WelcomeActionButtonProps) {
  const { theme } = useUnistyles();
  const Icon = action.icon;
  const buttonStyle = useMemo(
    () => [styles.actionButton, action.primary ? styles.actionButtonPrimary : null],
    [action.primary],
  );
  const textStyle = useMemo(
    () => [styles.actionText, action.primary ? styles.actionTextPrimary : null],
    [action.primary],
  );
  return (
    <Pressable
      style={buttonStyle}
      onPress={action.onPress}
      testID={action.testID}
      accessibilityRole="button"
      accessibilityLabel={action.label}
    >
      <Icon
        size={18}
        color={action.primary ? theme.colors.accentForeground : theme.colors.foreground}
      />
      <Text style={textStyle}>{action.label}</Text>
    </Pressable>
  );
}

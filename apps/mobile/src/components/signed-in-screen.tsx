import { useEarliestOnlineHostServerId } from "@/app/_layout";
import { AitLogo } from "@/components/icons/ait-logo";
import { Button } from "@/components/ui/button";
import { useAccountState } from "@/runtime/account-state";
import { supportsBrowserAccount } from "@/runtime/browser-account";
import { buildOpenProjectRoute } from "@/utils/host-routes";
import { useRouter } from "expo-router";
import { ArrowRight, CircleCheck, Settings } from "lucide-react-native";
import React, { useCallback, useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { ScrollView, Text, View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { StyleSheet, useUnistyles } from "react-native-unistyles";

const WELCOME_ROUTE = "/welcome";

/** Confirms a sign-in started from the welcome page and waits for an account host to connect. */
export function SignedInScreen() {
  const { theme } = useUnistyles();
  const { t } = useTranslation();
  const insets = useSafeAreaInsets();
  const router = useRouter();
  const account = useAccountState();
  const onlineServerId = useEarliestOnlineHostServerId();
  const signedOut = account.status === "logged_out";

  useEffect(() => {
    if (signedOut) router.replace(WELCOME_ROUTE);
  }, [router, signedOut]);

  const handleContinue = useCallback(() => {
    if (onlineServerId) router.replace(buildOpenProjectRoute());
  }, [onlineServerId, router]);
  const handleOtherConnection = useCallback(() => {
    router.replace(WELCOME_ROUTE);
  }, [router]);
  const handleOpenSettings = useCallback(() => {
    router.push("/settings");
  }, [router]);

  const containerStyle = useMemo(
    () => [styles.container, { paddingBottom: theme.spacing[6] + insets.bottom }],
    [theme.spacing, insets.bottom],
  );

  if (signedOut) return null;

  return (
    <View style={styles.root}>
      <ScrollView
        style={styles.scrollView}
        contentContainerStyle={containerStyle}
        showsVerticalScrollIndicator={false}
        testID="signed-in-screen"
      >
        <View style={styles.content}>
          <AitLogo size={120} variant="stacked" wordmarkColor={theme.colors.foreground} />
          <View style={styles.copyBlock}>
            <View style={styles.titleRow}>
              <CircleCheck size={20} color={theme.colors.statusDotSuccess} />
              <Text style={styles.title}>{t("onboarding.signedIn.title")}</Text>
            </View>
            {account.name ? (
              <Text style={styles.subtitle}>
                {t("onboarding.signedIn.account", { name: account.name })}
              </Text>
            ) : null}
          </View>

          <View style={styles.hostCard}>
            <Text style={styles.hostCardTitle}>{t("onboarding.signedIn.hostsTitle")}</Text>
            {account.hosts.length === 0 ? (
              <Text style={styles.hint}>{t("onboarding.signedIn.noHosts")}</Text>
            ) : (
              account.hosts.map((host) => (
                <View
                  key={host.host_id}
                  style={styles.hostRow}
                  testID={`signed-in-host-${host.host_id}`}
                >
                  <View style={[styles.statusDot, host.online ? styles.onlineDot : null]} />
                  <Text style={styles.hostName} numberOfLines={1}>
                    {host.name}
                  </Text>
                  <Text style={styles.hostStatus}>
                    {host.online
                      ? t("common.connectionStatus.online")
                      : t("onboarding.signedIn.offline")}
                  </Text>
                </View>
              ))
            )}
          </View>

          <View style={styles.actions}>
            <Button
              variant="default"
              leftIcon={ArrowRight}
              disabled={!onlineServerId}
              onPress={handleContinue}
              testID="signed-in-continue"
            >
              {t("onboarding.signedIn.continue")}
            </Button>
            {!onlineServerId ? (
              <Text style={styles.hint}>{t("onboarding.signedIn.waiting")}</Text>
            ) : null}
            {!supportsBrowserAccount() ? (
              <Button
                variant="ghost"
                size="sm"
                onPress={handleOtherConnection}
                testID="signed-in-other-connection"
              >
                {t("onboarding.signedIn.otherConnection")}
              </Button>
            ) : null}
            <Button
              variant="ghost"
              size="sm"
              leftIcon={Settings}
              onPress={handleOpenSettings}
              testID="signed-in-open-settings"
            >
              {t("onboarding.actions.settings")}
            </Button>
          </View>
        </View>
      </ScrollView>
    </View>
  );
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
    alignItems: "center",
  },
  content: {
    width: "100%",
    maxWidth: 420,
    flexGrow: 1,
    justifyContent: "center",
    alignItems: "center",
    gap: theme.spacing[8],
  },
  copyBlock: {
    alignItems: "center",
    gap: theme.spacing[2],
  },
  titleRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
  },
  title: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.lg,
    fontWeight: theme.fontWeight.medium,
  },
  subtitle: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.base,
    textAlign: "center",
  },
  hostCard: {
    width: "100%",
    gap: theme.spacing[3],
    padding: theme.spacing[4],
    borderRadius: theme.borderRadius.xl,
    backgroundColor: theme.colors.surface2,
    borderWidth: 1,
    borderColor: theme.colors.border,
  },
  hostCardTitle: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
  },
  hostRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
  },
  hostName: {
    flex: 1,
    minWidth: 0,
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
  },
  hostStatus: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
  statusDot: {
    width: 6,
    height: 6,
    borderRadius: 3,
    backgroundColor: theme.colors.foregroundMuted,
  },
  onlineDot: {
    backgroundColor: theme.colors.statusDotSuccess,
  },
  hint: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    textAlign: "center",
  },
  actions: {
    width: "100%",
    alignItems: "stretch",
    gap: theme.spacing[3],
  },
}));

import { isElectronRuntime } from "@/desktop/host";
import { AccountHostPanel } from "./account-host-panel";
import { supportsAccountRelay } from "@/runtime/account-state";
import type { Theme } from "@/styles/theme";
import { Globe, Link2, Terminal } from "lucide-react-native";
import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useRouter } from "expo-router";
import { useTranslation } from "react-i18next";
import { Pressable, Text, View } from "react-native";
import { StyleSheet, withUnistyles } from "react-native-unistyles";
import { AdaptiveModalSheet, type SheetHeader } from "./adaptive-modal-sheet";

const ThemedLink2 = withUnistyles(Link2);
const ThemedTerminal = withUnistyles(Terminal);
const ThemedGlobe = withUnistyles(Globe);
const foregroundIconMapping = (theme: Theme) => ({ color: theme.colors.foreground });

const styles = StyleSheet.create((theme) => ({
  option: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[4],
    padding: theme.spacing[4],
    borderRadius: theme.borderRadius.xl,
    backgroundColor: theme.colors.surface2,
    borderWidth: 1,
    borderColor: theme.colors.border,
  },
  optionText: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.normal,
  },
  optionSubtext: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.base,
    marginTop: theme.spacing[1],
  },
  optionBody: {
    flex: 1,
  },
}));

export interface AddHostMethodModalProps {
  visible: boolean;
  onClose: () => void;
  onDirectConnection: () => void;
  onRemoteSsh: () => void;
}

export function AddHostMethodModal({
  visible,
  onClose,
  onDirectConnection,
  onRemoteSsh,
}: AddHostMethodModalProps) {
  const { t } = useTranslation();
  const router = useRouter();
  const [showOnlineService, setShowOnlineService] = useState(false);
  useEffect(() => {
    if (!visible) setShowOnlineService(false);
  }, [visible]);
  const pendingAccountServerId = useRef<string | null>(null);
  const handleAccountConnected = useCallback(
    (serverId: string) => {
      pendingAccountServerId.current = serverId;
      onClose();
    },
    [onClose],
  );
  const handleDismiss = useCallback(() => {
    const serverId = pendingAccountServerId.current;
    pendingAccountServerId.current = null;
    if (serverId) router.push(`/h/${serverId}`);
  }, [router]);
  const header = useMemo<SheetHeader>(
    () =>
      showOnlineService
        ? {
            title: t("onlineService.title"),
            back: {
              onPress: () => setShowOnlineService(false),
              accessibilityLabel: t("common.back"),
            },
          }
        : { title: t("pairing.connectionMethods.title") },
    [showOnlineService, t],
  );

  const handleDirect = useCallback(() => {
    onDirectConnection();
  }, [onDirectConnection]);

  const handleRemoteSsh = useCallback(() => {
    onRemoteSsh();
  }, [onRemoteSsh]);

  return (
    <AdaptiveModalSheet
      header={header}
      visible={visible}
      onClose={onClose}
      onDismiss={handleDismiss}
      testID="add-host-method-modal"
    >
      {showOnlineService ? (
        <AccountHostPanel onConnected={handleAccountConnected} onCancel={onClose} />
      ) : (
        <>
          <Pressable
            style={styles.option}
            onPress={handleDirect}
            accessibilityRole="button"
            accessibilityLabel={t("pairing.connectionMethods.direct.title")}
            testID="add-host-method-direct"
          >
            <ThemedLink2 size={18} uniProps={foregroundIconMapping} />
            <View style={styles.optionBody}>
              <Text style={styles.optionText}>{t("pairing.connectionMethods.direct.title")}</Text>
              <Text style={styles.optionSubtext}>
                {t("pairing.connectionMethods.direct.description")}
              </Text>
            </View>
          </Pressable>

          {supportsAccountRelay() ? (
            <Pressable
              style={styles.option}
              onPress={() => setShowOnlineService(true)}
              accessibilityRole="button"
              accessibilityLabel={t("onlineService.title")}
              testID="add-host-method-online-service"
            >
              <ThemedGlobe size={18} uniProps={foregroundIconMapping} />
              <View style={styles.optionBody}>
                <Text style={styles.optionText}>{t("onlineService.title")}</Text>
                <Text style={styles.optionSubtext}>{t("onlineService.connectionDescription")}</Text>
              </View>
            </Pressable>
          ) : null}

          {isElectronRuntime() ? (
            <Pressable
              style={styles.option}
              onPress={handleRemoteSsh}
              accessibilityRole="button"
              accessibilityLabel={t("pairing.connectionMethods.remoteSsh.title")}
              testID="add-host-method-remote-ssh"
            >
              <ThemedTerminal size={18} uniProps={foregroundIconMapping} />
              <View style={styles.optionBody}>
                <Text style={styles.optionText}>
                  {t("pairing.connectionMethods.remoteSsh.title")}
                </Text>
                <Text style={styles.optionSubtext}>
                  {t("pairing.connectionMethods.remoteSsh.description")}
                </Text>
              </View>
            </Pressable>
          ) : null}
        </>
      )}
    </AdaptiveModalSheet>
  );
}

import { useCallback } from "react";
import { router } from "expo-router";
import { shouldUseDesktopDaemon } from "@/desktop/daemon/desktop-daemon";
import { useLocalDaemonServerIdState } from "@/hooks/use-is-local-daemon";
import { useHosts } from "@/runtime/host-runtime";
import { useDesktopSettings } from "@/desktop/settings/desktop-settings";
import { useDaemonStatus } from "@/desktop/hooks/use-daemon-status";
import { useBuiltInDaemonManagement } from "@/desktop/hooks/use-built-in-daemon-management";
import { buildSettingsHostSectionRoute } from "@/utils/host-routes";

export interface EnableBuiltInDaemonOption {
  visible: boolean;
  onPress: () => void;
}

export function useEnableBuiltInDaemonOption(): EnableBuiltInDaemonOption {
  const supportsBuiltInDaemon = shouldUseDesktopDaemon();
  const localDaemon = useLocalDaemonServerIdState();
  const hosts = useHosts();
  const { settings, updateSettings } = useDesktopSettings();
  const { data, setStatus, refetch } = useDaemonStatus();
  const { enable } = useBuiltInDaemonManagement({
    daemonStatus: data?.status ?? null,
    settings: settings.daemon,
    updateSettings: (updates) => updateSettings({ daemon: updates }),
    setStatus,
    refreshStatus: refetch,
  });

  const isLocalhostConfigured =
    localDaemon.status === "resolved" &&
    localDaemon.serverId !== null &&
    hosts.some((host) => host.serverId === localDaemon.serverId);
  const visible =
    supportsBuiltInDaemon && localDaemon.status === "resolved" && !isLocalhostConfigured;

  const onPress = useCallback(() => {
    void (async () => {
      const result = await enable();
      if (result?.kind === "enabled") {
        router.push(buildSettingsHostSectionRoute(result.newStatus.serverId, "host"));
      }
    })();
  }, [enable]);

  return { visible, onPress };
}

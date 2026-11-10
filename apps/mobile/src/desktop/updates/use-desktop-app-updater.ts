import { useCallback, useEffect, useSyncExternalStore } from "react";
import {
  checkDesktopAppUpdate,
  formatVersionWithPrefix,
  installDesktopAppUpdate,
  shouldShowDesktopUpdateSection,
  type DesktopAppUpdateCheckResult,
  type DesktopAppUpdateCheckIntent,
  type DesktopAppUpdateInstallResult,
} from "@/desktop/updates/desktop-updates";
import { useDesktopSettings } from "@/desktop/settings/desktop-settings";
import { useDesktopIpcErrorReporter } from "@/desktop/hooks/desktop-ipc-error";
import {
  createDesktopAppUpdater,
  formatStatusText,
  type DesktopAppUpdater,
  type DesktopAppUpdaterErrorReport,
  type DesktopAppUpdateStatus,
} from "@/desktop/updates/desktop-app-updater";
import { formatMessageTimestamp } from "@/utils/time";

export type { DesktopAppUpdateStatus };

export interface UseDesktopAppUpdaterReturn {
  isDesktopApp: boolean;
  status: DesktopAppUpdateStatus;
  statusText: string;
  availableUpdate: DesktopAppUpdateCheckResult | null;
  errorMessage: string | null;
  lastCheckedAt: number | null;
  isChecking: boolean;
  isInstalling: boolean;
  checkForUpdates: (options?: {
    intent?: DesktopAppUpdateCheckIntent;
    silent?: boolean;
  }) => Promise<DesktopAppUpdateCheckResult | null>;
  installUpdate: () => Promise<DesktopAppUpdateInstallResult | null>;
}

// One updater per app: the sidebar callout and Settings must observe the same check/install
// state, and the install guard only works if both drive the same instance.
let sharedUpdater: DesktopAppUpdater | null = null;
let reportSharedInstallError: ((report: DesktopAppUpdaterErrorReport) => void) | null = null;

function getSharedDesktopAppUpdater(): DesktopAppUpdater {
  sharedUpdater ??= createDesktopAppUpdater({
    port: {
      checkDesktopAppUpdate,
      installDesktopAppUpdate,
    },
    now: () => Date.now(),
    reportInstallError: (report) => reportSharedInstallError?.(report),
  });
  return sharedUpdater;
}

export function useDesktopAppUpdater(): UseDesktopAppUpdaterReturn {
  const isDesktopApp = shouldShowDesktopUpdateSection();
  const { settings: desktopSettings, isLoading: isLoadingSettings } = useDesktopSettings();
  const releaseChannel = desktopSettings.releaseChannel;
  const reportError = useDesktopIpcErrorReporter();
  const updater = getSharedDesktopAppUpdater();

  useEffect(() => {
    reportSharedInstallError = reportError;
  }, [reportError]);

  const snapshot = useSyncExternalStore(
    updater.subscribe,
    updater.getSnapshot,
    updater.getSnapshot,
  );

  const checkForUpdates = useCallback(
    async (options: { intent?: DesktopAppUpdateCheckIntent; silent?: boolean } = {}) => {
      if (!isDesktopApp) {
        return null;
      }
      const intent = options.intent ?? "manual";
      // Until settings load, releaseChannel is the "stable" default; an automatic check would
      // query the wrong channel.
      if (intent === "automatic" && isLoadingSettings) {
        return null;
      }
      return updater.checkForUpdates({
        releaseChannel,
        intent,
        silent: options.silent,
      });
    },
    [isDesktopApp, isLoadingSettings, releaseChannel, updater],
  );

  const installUpdate = useCallback(async () => {
    if (!isDesktopApp) {
      return null;
    }
    return updater.installUpdate({ releaseChannel });
  }, [isDesktopApp, releaseChannel, updater]);

  // Automatic checks run on the shared updater's single schedule; each mounted caller only keeps
  // it alive. Until settings load, releaseChannel is the "stable" default and would check the
  // wrong channel.
  useEffect(() => {
    if (!isDesktopApp || isLoadingSettings) {
      return undefined;
    }
    return updater.retainAutomaticChecks(releaseChannel);
  }, [isDesktopApp, isLoadingSettings, releaseChannel, updater]);

  return {
    isDesktopApp,
    status: snapshot.status,
    statusText: formatStatusText({
      status: snapshot.status,
      availableUpdate: snapshot.availableUpdate,
      installMessage: snapshot.installMessage,
      lastCheckedAt: snapshot.lastCheckedAt,
      formatVersion: formatVersionWithPrefix,
      formatLastCheckedAt: (timestamp) => formatMessageTimestamp(new Date(timestamp)),
    }),
    availableUpdate: snapshot.availableUpdate,
    errorMessage: snapshot.errorMessage,
    lastCheckedAt: snapshot.lastCheckedAt,
    isChecking: snapshot.isChecking,
    isInstalling: snapshot.isInstalling,
    checkForUpdates,
    installUpdate,
  };
}

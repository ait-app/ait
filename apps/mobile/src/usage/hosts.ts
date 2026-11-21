import { resolveHostGroupServerId } from "@/hosts/host-groups";
import { useCallback } from "react";
import { useHosts } from "@/runtime/host-runtime";
import { useAppSettings } from "@/hooks/use-settings";
import {
  useActiveWorkspaceSelection,
  useLastWorkspaceSelection,
} from "@/stores/navigation-active-workspace-store";
import { resolveUsageHostId, resolveUsageSettingsHostId, type UsageHostChoice } from "./model";
import { setUsageHost } from "./preferences";
import { useUsageHosts } from "./queries";

/** The active workspace's host; off a workspace route, the last workspace visited. */
function useActiveServerId(): string | null {
  const active = useActiveWorkspaceSelection();
  const last = useLastWorkspaceSelection();
  return active?.serverId ?? last?.serverId ?? null;
}

function useUsageHostChoice(): UsageHostChoice {
  const { settings } = useAppSettings();
  const hosts = useHosts();
  const activeServerId = useActiveServerId();
  return {
    pickedServerId: settings.usage.serverId
      ? resolveHostGroupServerId(hosts, settings.usage.serverId)
      : null,
    activeServerId: activeServerId ? resolveHostGroupServerId(hosts, activeServerId) : null,
    hosts: useUsageHosts(),
  };
}

/** The host the sidebar Usage row reads, or null when none reports usage. */
export function useUsageHostId(): string | null {
  return resolveUsageHostId(useUsageHostChoice());
}

/** The host whose Usage page the sidebar opens in Settings, or null when there are no hosts. */
export function useUsageSettingsHostId(): string | null {
  return resolveUsageSettingsHostId(useUsageHostChoice());
}

/**
 * Makes a host the one the sidebar Usage summary reads. The pick is saved on the device, so the
 * sidebar and later visits keep showing that host.
 */
export function useSelectUsageHost(): (serverId: string) => void {
  const { updateSettings } = useAppSettings();
  return useCallback(
    (serverId: string) => {
      // updateSettings logs a failed save before rethrowing; only the rejection needs handling.
      updateSettings((current) => ({ usage: setUsageHost(current.usage, serverId) })).catch(
        () => {},
      );
    },
    [updateSettings],
  );
}

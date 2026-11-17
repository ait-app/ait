import { useCallback, useMemo } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { MutableDaemonConfig, MutableDaemonConfigPatch } from "@ait/protocol/messages";
import { useReplicaQuery } from "@/data/query";
import { applyDaemonConfigPatch, daemonConfigQueryKey } from "@/data/daemon-config";
import { useDebouncedValue } from "@/hooks/use-debounced-value";
import { useHostRuntimeClient, useHostRuntimeIsConnected } from "@/runtime/host-runtime";

/**
 * How long "not connected" must persist before it surfaces as a config error.
 *
 * A reconnect swaps the client and dips the connection flag for a moment. Reporting that
 * immediately renders a red banner (and `HostConfigError`) for a host that is coming right
 * back, which reads as "saved, but broken" and clears itself a blink later.
 */
const DISCONNECT_ERROR_DELAY_MS = 1_000;

interface UseDaemonConfigResult {
  config: MutableDaemonConfig | null;
  isLoading: boolean;
  error: Error | null;
  retry: () => void;
  patchConfig: (patch: MutableDaemonConfigPatch) => Promise<MutableDaemonConfig | undefined>;
}

export function useDaemonConfig(serverId: string | null): UseDaemonConfigResult {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const client = useHostRuntimeClient(serverId ?? "");
  const isConnected = useHostRuntimeIsConnected(serverId ?? "");
  const queryKey = useMemo(() => daemonConfigQueryKey(serverId), [serverId]);

  const configQuery = useReplicaQuery({
    queryKey,
    enabled: Boolean(serverId && client && isConnected),
    pushEvent: "status:daemon_config_changed",
    queryFn: async () => {
      if (!client) {
        throw new Error(t("workspace.terminal.hostDisconnected"));
      }
      const result = await client.getDaemonConfig();
      return result.config;
    },
  });

  const patchConfig = useCallback(
    async (patch: MutableDaemonConfigPatch) => {
      if (!client || !isConnected) {
        throw new Error(t("workspace.terminal.hostDisconnected"));
      }
      // Show the change before the round trip. A daemon patch costs a durability write, so
      // without this a toggle renders the old value until the response lands, which reads as
      // "the tap did nothing" and then as a value that jumped.
      const previous = queryClient.getQueryData<MutableDaemonConfig>(queryKey);
      const optimistic = previous ? applyDaemonConfigPatch(previous, patch) : undefined;
      const applied = optimistic
        ? queryClient.setQueryData<MutableDaemonConfig>(queryKey, optimistic)
        : undefined;
      try {
        const result = await client.patchDaemonConfig(patch);
        queryClient.setQueryData(queryKey, result.config);
        return result.config;
      } catch (error) {
        // Roll back only while the cache still holds this call's optimistic value: a newer
        // patch or push already owns the cache otherwise, and rewriting it here would discard
        // that newer state.
        if (applied && queryClient.getQueryData(queryKey) === applied) {
          queryClient.setQueryData(queryKey, previous);
        }
        throw error;
      }
    },
    [client, isConnected, queryClient, queryKey, t],
  );

  // Only a settled disconnection is an error; the client and query preserve the last config
  // across a reconnect, so a transient dip must not render as a failure.
  const connectedLongEnough = useDebouncedValue(isConnected, DISCONNECT_ERROR_DELAY_MS);
  const disconnected = !isConnected && !connectedLongEnough;

  return {
    config: configQuery.data ?? null,
    isLoading: configQuery.isLoading,
    error:
      configQuery.error ??
      (disconnected ? new Error(t("workspace.terminal.hostDisconnected")) : null),
    retry: () => {
      void configQuery.refetch();
    },
    patchConfig,
  };
}

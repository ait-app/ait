import { useQuery, type QueryClient } from "@tanstack/react-query";
import type { AgentProvider } from "@ait/protocol/agent-types";
import type { ProviderPreset, ProviderPresetDescriptor } from "@ait/protocol/messages";
import { useHostRuntimeClient, useHostRuntimeIsConnected } from "@/runtime/host-runtime";

/** One provider's preset store, as the composer and the settings section both read it. */
export interface ProviderPresetsSnapshot {
  descriptor: ProviderPresetDescriptor;
  presets: ProviderPreset[];
}

/** Cache identity for one provider's presets, scoped by host and working directory. */
export function providerPresetsQueryKey(
  serverId: string | null | undefined,
  provider: string | null | undefined,
  cwd: string | null | undefined,
) {
  return ["provider-presets", serverId ?? null, provider ?? null, cwd ?? null] as const;
}

/**
 * Drop every cache a preset write invalidates.
 *
 * The editor itself reads `provider-presets`, but the selectors that must show the new entry
 * read the provider's feature projection: the draft form, the profile form and a live agent's
 * role choices each own a separate query. Invalidating only the editor's own key would leave
 * those pickers showing the old list, so all four are dropped together.
 */
export async function invalidatePresetCaches(
  queryClient: QueryClient,
  serverId: string,
  provider: string,
) {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ["provider-presets", serverId, provider] }),
    queryClient.invalidateQueries({ queryKey: ["providerFeatures", serverId, provider] }),
    queryClient.invalidateQueries({ queryKey: ["agentProfileFeatures", serverId] }),
    queryClient.invalidateQueries({ queryKey: ["providerRoleChoices", serverId] }),
  ]);
}

/**
 * Read a provider's preset store once per host/provider/directory.
 *
 * The editor and every composer share this entry, so opening a picker never repeats the
 * request. A provider whose host predates the preset RPCs, or whose store cannot be read,
 * yields `null` and callers render nothing rather than a control that cannot work.
 */
export function useProviderPresets(input: {
  serverId: string | null | undefined;
  provider: string | null | undefined;
  cwd: string | null | undefined;
  enabled?: boolean;
}): ProviderPresetsSnapshot | null {
  const { serverId, provider, cwd, enabled = true } = input;
  const client = useHostRuntimeClient(serverId ?? "");
  const isConnected = useHostRuntimeIsConnected(serverId ?? "");
  const available = Boolean(enabled && serverId && provider && client && isConnected);

  const query = useQuery({
    queryKey: providerPresetsQueryKey(serverId, provider, cwd),
    enabled: available,
    staleTime: 60 * 1000,
    // A missing capability is a stable fact for the session, so never hammer the host.
    retry: false,
    queryFn: async (): Promise<ProviderPresetsSnapshot | null> => {
      if (!client || !provider) return null;
      const described = await client.describeProviderPresets(provider, cwd ?? undefined);
      const listed = await client.listProviderPresets(provider, cwd ?? undefined);
      return { descriptor: described.descriptor, presets: listed.presets };
    },
  });

  return query.data ?? null;
}

/**
 * Whether the preset control must be shown for this provider at all.
 *
 * A store that exists but is empty still counts: the editor is the only way to fill it, and the
 * empty state is what points there.
 */
export function presetControlAvailable(
  snapshot: ProviderPresetsSnapshot | null | undefined,
): boolean {
  return snapshot !== null && snapshot !== undefined;
}

/** Host/provider/directory whose preset editor the empty state opens. */
export interface PresetSettingsTarget {
  serverId: string | null | undefined;
  provider: AgentProvider | null | undefined;
  cwd: string | null | undefined;
}

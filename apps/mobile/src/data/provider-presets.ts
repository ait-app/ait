import { useQuery, type QueryClient } from "@tanstack/react-query";
import type { AgentProvider } from "@ait/protocol/agent-types";
import type { ProviderPreset, ProviderPresetDescriptor } from "@ait/protocol/messages";
import { useHostRuntimeClient, useHostRuntimeIsConnected } from "@/runtime/host-runtime";

/** One provider's preset store, as the composer and the settings section both read it. */
export interface ProviderPresetsSnapshot {
  descriptor: ProviderPresetDescriptor;
  presets: ProviderPreset[];
}

/** One store answer: the cached list plus whatever the store itself reported. */
interface PresetStoreAnswer {
  snapshot: ProviderPresetsSnapshot;
  /** Why the store could not list anything, when it answered with a reason instead of failing. */
  error: string | null;
}

/** The shared store query, plus the flags its two readers render differently. */
export interface ProviderPresetsState {
  /** `null` until the host answers, or on a host that predates the preset RPCs. */
  snapshot: ProviderPresetsSnapshot | null;
  isLoading: boolean;
  error: string | null;
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
 * those pickers showing the old list, so all four are dropped together. Active queries refetch
 * on invalidation, so a mounted editor updates without a second manual read.
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
 * The editor and the empty state share this entry, so opening a picker never repeats the
 * request and a write's invalidation reaches both. A provider whose host predates the preset
 * RPCs, or whose store cannot be read, yields `null` and callers render nothing rather than a
 * control that cannot work.
 */
export function useProviderPresets(input: {
  serverId: string | null | undefined;
  provider: string | null | undefined;
  cwd: string | null | undefined;
  enabled?: boolean;
}): ProviderPresetsState {
  const { serverId, provider, cwd, enabled = true } = input;
  const client = useHostRuntimeClient(serverId ?? "");
  const isConnected = useHostRuntimeIsConnected(serverId ?? "");
  const available = Boolean(enabled && serverId && provider && client && isConnected);

  const query = useQuery({
    queryKey: providerPresetsQueryKey(serverId, provider, cwd),
    enabled: available,
    // A store is small and changes only through this app, so a write's invalidation keeps it
    // fresh; rereading between writes only costs a native probe on OMP.
    staleTime: 60 * 1000,
    // A missing capability is a stable fact for the session, so never hammer the host.
    retry: false,
    queryFn: async (): Promise<PresetStoreAnswer | null> => {
      if (!client || !provider) return null;
      const described = await client.describeProviderPresets(provider, cwd ?? undefined);
      const listed = await client.listProviderPresets(provider, cwd ?? undefined);
      return {
        snapshot: { descriptor: described.descriptor, presets: listed.presets },
        error: listed.error ?? null,
      };
    },
  });

  return {
    // The descriptor survives a store that refused to list, so the editor can still explain the
    // store and withhold writes instead of showing an empty list with no reason.
    snapshot: query.data?.snapshot ?? null,
    isLoading: available && query.isLoading,
    // A store reason is part of a successful answer; a transport failure replaces it.
    error: query.error instanceof Error ? query.error.message : (query.data?.error ?? null),
  };
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

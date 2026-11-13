import { useQuery, type QueryClient } from "@tanstack/react-query";
import type { AgentFeature, AgentProvider } from "@ait/protocol/agent-types";
import type { ProviderPresetEntry, ProviderPresetDescriptor } from "@ait/protocol/messages";
import { useHostRuntimeClient, useHostRuntimeIsConnected } from "@/runtime/host-runtime";

/** One provider's preset store, as the settings section reads it. */
export interface ProviderPresetsSnapshot {
  descriptor: ProviderPresetDescriptor;
  presets: ProviderPresetEntry[];
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

export function providerPresetFeaturesKey(serverId: string, provider: string, cwd?: string | null) {
  return ["providerPresetFeatures", serverId, provider, cwd ?? null] as const;
}

/** Role candidates belong to the same provider and directory as the records being edited. */
export function useProviderPresetFeatures(serverId: string, provider: string, cwd?: string) {
  const client = useHostRuntimeClient(serverId);
  const connected = useHostRuntimeIsConnected(serverId);
  return useQuery({
    queryKey: providerPresetFeaturesKey(serverId, provider, cwd),
    enabled: Boolean(client && connected),
    staleTime: 60_000,
    retry: false,
    queryFn: async (): Promise<AgentFeature[]> => {
      if (!client) throw new Error("Host disconnected");
      const result = await client.listProviderFeatures({
        provider,
        cwd: cwd ?? "~",
      });
      if (result.error) throw new Error(result.error);
      return result.features ?? [];
    },
  });
}

/** Explicit refresh retries even failed stores and reports transport and native list failures. */
export async function refreshProviderPresetSettings(
  queryClient: QueryClient,
  target: { serverId: string; provider: string; cwd?: string },
  modelsRefreshed: Promise<unknown> = Promise.resolve(),
) {
  const { serverId, provider, cwd } = target;
  const key = providerPresetsQueryKey(serverId, provider, cwd);
  const [store] = await Promise.allSettled([
    queryClient.invalidateQueries({ queryKey: key, exact: true }, { throwOnError: true }),
    modelsRefreshed,
  ]);
  // Feature discovery must observe the acknowledged new model catalog, not race its push.
  const results = await Promise.allSettled([
    queryClient.invalidateQueries(
      { queryKey: providerPresetFeaturesKey(serverId, provider, cwd), exact: true },
      { throwOnError: true },
    ),
    queryClient.invalidateQueries(
      { queryKey: ["providerFeatures", serverId, provider, cwd ?? null] },
      { throwOnError: true },
    ),
    queryClient.invalidateQueries(
      { queryKey: ["providerRoleChoices", serverId, cwd] },
      { throwOnError: true },
    ),
    queryClient.invalidateQueries(
      {
        predicate: (query) =>
          query.queryKey[0] === "agentProfileFeatures" &&
          query.queryKey[1] === serverId &&
          String(query.queryKey[2]).startsWith(`${provider}|`),
      },
      { throwOnError: true },
    ),
  ]);
  if (store.status === "rejected") throw store.reason;
  const failed = results.find((result) => result.status === "rejected");
  if (failed?.status === "rejected") throw failed.reason;
  const answer = queryClient.getQueryData<PresetStoreAnswer>(key);
  if (answer?.error) throw new Error(answer.error);
}

/**
 * Drop every cache a preset write invalidates.
 *
 * The editor itself reads `provider-presets`, but the configuration controls that must show the new entry
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
    queryClient.invalidateQueries({ queryKey: ["providerPresetFeatures", serverId, provider] }),
    queryClient.invalidateQueries({ queryKey: ["providerFeatures", serverId, provider] }),
    queryClient.invalidateQueries({ queryKey: ["agentProfileFeatures", serverId] }),
    queryClient.invalidateQueries({ queryKey: ["providerRoleChoices", serverId] }),
  ]);
}

/**
 * Read a provider's preset store once per host/provider/directory.
 *
 * The settings list and its empty state share this entry, so opening the page never repeats the
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
    isLoading: available && query.isFetching,
    // A store reason is part of a successful answer; a transport failure replaces it.
    error: query.error instanceof Error ? query.error.message : (query.data?.error ?? null),
  };
}

/** Host/provider/directory whose settings the model picker gear opens. */
export interface PresetSettingsTarget {
  serverId: string | null | undefined;
  provider: AgentProvider | null | undefined;
  cwd: string | null | undefined;
}

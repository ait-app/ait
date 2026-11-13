import { CancelledError, type QueryClient } from "@tanstack/react-query";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import type { GetProvidersSnapshotResponseMessage } from "@ait/protocol/messages";
import { compactProviderSnapshot } from "@ait/protocol/provider-snapshot-codec";
import {
  providerSnapshotCache,
  ProviderSnapshotCacheMissError,
  type ProviderSnapshotCache,
} from "./provider-snapshot-cache";
import { queryClient as singletonQueryClient } from "./query-client";
import { replaceProviderSnapshotIcons } from "@/components/provider-icon-name";
import { agentCommandsQueryRoot } from "@/hooks/agent-commands-query";
import type { AgentProvider } from "@ait/protocol/agent-types";
import { normalizeWorkspacePath } from "@/utils/workspace-identity";

export const PROVIDERS_SNAPSHOT_QUERY_ROOT = "providersSnapshot";

export function normalizeProvidersSnapshotCwd(cwd?: string | null): string | null {
  return normalizeWorkspacePath(cwd);
}

export function providersSnapshotQueryRoot(serverId: string | null) {
  return [PROVIDERS_SNAPSHOT_QUERY_ROOT, serverId] as const;
}

export function providersSnapshotQueryKey(serverId: string | null, cwd?: string | null) {
  const normalizedCwd = normalizeProvidersSnapshotCwd(cwd);
  return normalizedCwd
    ? ([PROVIDERS_SNAPSHOT_QUERY_ROOT, serverId, "cwd", normalizedCwd] as const)
    : ([PROVIDERS_SNAPSHOT_QUERY_ROOT, serverId, "home"] as const);
}

export function providersSnapshotRequestOptions(input: {
  cwd?: string | null;
  providers?: AgentProvider[];
  ifNoneMatch?: string;
}) {
  const normalizedCwd = normalizeProvidersSnapshotCwd(input.cwd);
  return {
    ...(normalizedCwd ? { cwd: normalizedCwd } : {}),
    ...(input.providers ? { providers: input.providers } : {}),
    ...(input.ifNoneMatch ? { ifNoneMatch: input.ifNoneMatch } : {}),
  };
}

export function isProvidersSnapshotHomeScope(cwd?: string | null): boolean {
  return normalizeProvidersSnapshotCwd(cwd) === null;
}

type Snapshot = GetProvidersSnapshotResponseMessage["payload"];
type SnapshotClient = Pick<DaemonClient, "getProvidersSnapshot">;

function withSnapshotBody(snapshot: Snapshot): Snapshot {
  if (!snapshot.snapshotHash || snapshot.compactSnapshot || snapshot.notModified) return snapshot;
  // A full Rust daemon response carries entries and a hash without a compact body.
  return { ...snapshot, compactSnapshot: compactProviderSnapshot(snapshot.entries) };
}

export async function fetchProvidersSnapshot(input: {
  client: SnapshotClient;
  serverId: string;
  cwd: string | null;
  queryClient?: QueryClient;
  cache?: ProviderSnapshotCache;
  signal?: AbortSignal;
  snapshot?: Snapshot;
}): Promise<Snapshot> {
  const cache = input.cache ?? providerSnapshotCache;
  const queryClient = input.queryClient ?? singletonQueryClient;
  const queryKey = providersSnapshotQueryKey(input.serverId, input.cwd);
  const cached = await cache.read(input.serverId, input.cwd);
  if (input.signal?.aborted) throw new CancelledError();
  const response =
    input.snapshot ??
    (await input.client.getProvidersSnapshot(
      providersSnapshotRequestOptions({ cwd: input.cwd, ifNoneMatch: cached?.hash }),
    ));
  if (input.signal?.aborted) throw new CancelledError();
  // Empty push entries may announce a body reference; an unconditional GET is a full body,
  // including when the host has no providers. A not-modified GET still requires its cached body.
  let snapshot =
    !input.snapshot || response.entries.length > 0 ? withSnapshotBody(response) : response;
  if (snapshot.snapshotHash && !snapshot.compactSnapshot) {
    const known =
      cached?.hash === snapshot.snapshotHash
        ? cached
        : await cache.readHash(input.serverId, snapshot.snapshotHash);
    if (known) snapshot = { ...snapshot, compactSnapshot: known.compactSnapshot };
    else if (snapshot.notModified) throw new ProviderSnapshotCacheMissError();
    else {
      const hash = snapshot.snapshotHash;
      const body = await queryClient.fetchQuery({
        queryKey: ["providerSnapshotContent", input.serverId, hash],
        gcTime: 0,
        structuralSharing: false,
        staleTime: 0,
        retry: false,
        queryFn: async () => {
          const response = await input.client.getProvidersSnapshot(
            providersSnapshotRequestOptions({ cwd: input.cwd }),
          );
          // Materialize before settling so simultaneous directory announcements share
          // both the transfer and decoded body through the existing query deduplication.
          if (input.signal?.aborted) throw new CancelledError();
          return cache.materialize(input.serverId, withSnapshotBody(response), input.signal);
        },
      });
      if (body.snapshotHash === hash) {
        snapshot = { ...snapshot, compactSnapshot: body.compactSnapshot };
      } else if (normalizeProvidersSnapshotCwd(body.cwd) === input.cwd) {
        snapshot = body;
      } else {
        snapshot = withSnapshotBody(
          await input.client.getProvidersSnapshot(
            providersSnapshotRequestOptions({ cwd: input.cwd }),
          ),
        );
      }
    }
  }
  if (input.signal?.aborted) throw new CancelledError();
  snapshot = await cache.materialize(input.serverId, snapshot, input.signal);
  if (input.signal?.aborted) throw new CancelledError();
  const current = queryClient.getQueryData<Snapshot>(queryKey);
  if (isNewerSnapshot(current, snapshot)) return current;
  const previous = (current as Snapshot | undefined)?.entries ?? [];
  let retained = false;
  const entries = snapshot.entries.map((entry) => {
    if ((entry.status !== "error" && entry.status !== "unavailable") || entry.models?.length) {
      return entry;
    }
    const lastGood =
      previous.find(
        (candidate) => candidate.provider === entry.provider && candidate.models?.length,
      ) ??
      cached?.entries.find(
        (candidate) => candidate.provider === entry.provider && candidate.models?.length,
      );
    if (!lastGood) return entry;
    retained = true;
    return { ...entry, models: lastGood.models, modes: lastGood.modes };
  });
  // Do not associate a synthesized body with a host content hash or replace
  // the last successful disk body with a failed discovery.
  if (!retained && snapshot.compactSnapshot && snapshot.snapshotHash) {
    await cache.write({
      serverId: input.serverId,
      cwd: input.cwd,
      hash: snapshot.snapshotHash,
      generatedAt: snapshot.generatedAt,
      fetchedAt: snapshot.fetchedAt,
      compactSnapshot: snapshot.compactSnapshot,
      signal: input.signal,
    });
  }
  if (input.signal?.aborted) throw new CancelledError();
  const latest = queryClient.getQueryData<Snapshot>(queryKey);
  if (isNewerSnapshot(latest, snapshot)) return latest;
  replaceProviderSnapshotIcons(input.serverId, entries);
  return retained ? { ...snapshot, entries } : snapshot;
}

export async function refreshAndApplyProvidersSnapshot(input: {
  client: SnapshotClient & Pick<DaemonClient, "refreshProvidersSnapshot">;
  queryClient: QueryClient;
  serverId: string;
  cwd: string | null;
  providers?: AgentProvider[];
  cache?: ProviderSnapshotCache;
  signal?: AbortSignal;
}) {
  if (input.signal?.aborted) throw new CancelledError();
  const result = await input.client.refreshProvidersSnapshot(
    providersSnapshotRequestOptions(input),
  );
  if (input.signal?.aborted) throw new CancelledError();
  const queryKey = providersSnapshotQueryKey(input.serverId, input.cwd);
  await input.queryClient.cancelQueries({ queryKey, exact: true });
  const signal = input.signal ?? new AbortController().signal;
  if (signal.aborted) throw new CancelledError();
  const deadline = Date.now() + 120_000;
  while (true) {
    const snapshot = await fetchProvidersSnapshot({ ...input, signal });
    if (signal.aborted) throw new CancelledError();
    const pending = snapshot.refreshing?.some(
      (provider) => !input.providers || input.providers.includes(provider),
    );
    input.queryClient.setQueryData(queryKey, snapshot);
    if (!pending) break;
    if (Date.now() >= deadline) throw new Error("Provider refresh timed out");
    await waitForRefresh(signal);
  }
  void input.queryClient.invalidateQueries({
    queryKey: agentCommandsQueryRoot(input.serverId),
    exact: false,
  });
  if (isProvidersSnapshotHomeScope(input.cwd)) {
    void input.queryClient.invalidateQueries({
      queryKey: providersSnapshotQueryRoot(input.serverId),
      exact: false,
    });
  }
  return result;
}

/**
 * Selector access joins discovery already in progress and shares each native
 * refresh with other selectors for the same host and directory.
 */
export async function refreshProvidersOnAccess(input: {
  client: SnapshotClient & Pick<DaemonClient, "refreshProvidersSnapshot">;
  queryClient: QueryClient;
  serverId: string;
  cwd: string | null;
  providers?: AgentProvider[];
  isCurrent?: () => boolean;
  isScopeCurrent?: () => boolean;
  subscribe?: (callback: () => void) => () => void;
}) {
  const queryKey = providersSnapshotQueryKey(input.serverId, input.cwd);
  const snapshot = input.queryClient.getQueryData<Snapshot>(queryKey);
  if (!snapshot) {
    await input.queryClient.refetchQueries({ queryKey, exact: true, type: "active" });
    return;
  }
  const requested = new Set(
    input.providers ??
      snapshot.entries.filter((entry) => entry.enabled).map((entry) => entry.provider),
  );
  const refreshKey = ["providersSnapshotRefresh", input.serverId, input.cwd] as const;
  while (requested.size) {
    const current = input.queryClient.getQueryData<Snapshot>(queryKey);
    const batch = [...requested].filter((provider) => {
      const entry = current?.entries.find((candidate) => candidate.provider === provider);
      return entry && entry.status !== "loading" && !current?.refreshing?.includes(provider);
    });
    if (!batch.length) return;
    const completed = await input.queryClient.fetchQuery({
      queryKey: refreshKey,
      gcTime: 0,
      retry: false,
      queryFn: async ({ signal }) => {
        const controller = new AbortController();
        const abort = () => controller.abort();
        signal.addEventListener("abort", abort, { once: true });
        let unsubscribe = () => {};
        const checkOwner = () => {
          // The caller's host identity or its active directory may change while
          // the native refresh is in flight; a stale scope must not publish.
          if (
            (input.isCurrent && !input.isCurrent()) ||
            (input.isScopeCurrent && !input.isScopeCurrent())
          ) {
            controller.abort();
            unsubscribe();
            void input.queryClient.cancelQueries({ queryKey: refreshKey, exact: true });
          }
        };
        unsubscribe = input.subscribe?.(checkOwner) ?? unsubscribe;
        checkOwner();
        if (signal.aborted) controller.abort();
        try {
          await refreshAndApplyProvidersSnapshot({
            ...input,
            providers: batch,
            signal: controller.signal,
          });
          return batch;
        } catch (error) {
          if (!controller.signal.aborted && !(error instanceof CancelledError)) {
            input.queryClient.setQueryData<Snapshot>(queryKey, (latest) => {
              if (!latest) return latest;
              const message = error instanceof Error ? error.message : String(error);
              return {
                ...latest,
                entries: latest.entries.map((entry) =>
                  batch.includes(entry.provider)
                    ? { ...entry, status: "error", error: message }
                    : entry,
                ),
              };
            });
          }
          throw error;
        } finally {
          unsubscribe();
          signal.removeEventListener("abort", abort);
        }
      },
    });
    for (const provider of completed) requested.delete(provider);
  }
}

function isNewerSnapshot(current: Snapshot | undefined, incoming: Snapshot): current is Snapshot {
  return (
    current?.generation !== undefined &&
    current.generation === incoming.generation &&
    current.revision !== undefined &&
    incoming.revision !== undefined &&
    current.revision > incoming.revision
  );
}

function waitForRefresh(signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal.aborted) {
      reject(new CancelledError());
      return;
    }
    const cancel = () => {
      clearTimeout(timer);
      reject(new CancelledError());
    };
    const timer = setTimeout(() => {
      signal.removeEventListener("abort", cancel);
      resolve();
    }, 200);
    signal.addEventListener("abort", cancel, { once: true });
  });
}

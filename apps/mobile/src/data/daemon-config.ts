import type { MutableDaemonConfig, MutableDaemonConfigPatch } from "@ait/protocol/messages";

export function daemonConfigQueryKey(serverId: string | null) {
  return ["daemon-config", serverId] as const;
}

/**
 * Overlay a patch object on the current one.
 *
 * The patch carries only the keys it changes, so the merged value keeps the field's own type.
 */
function mergeField<T>(current: T | undefined, patch: object): T {
  return Object.assign({}, current, patch) as T;
}

/**
 * Merge one provider entry the way the daemon does.
 *
 * `providers` is deep-merged because each entry is owned independently: a patch may carry only
 * `enabled` for one provider, and a new provider arrives as one whole entry. `env` and `params`
 * are the only nested objects a provider owns, so they merge rather than replace.
 */
function mergeProviderEntry(
  current: Record<string, unknown> | undefined,
  patch: Record<string, unknown>,
): Record<string, unknown> {
  const next: Record<string, unknown> = { ...current, ...patch };
  for (const field of ["env", "params"] as const) {
    const patchValue = patch[field];
    if (typeof patchValue === "object" && patchValue !== null && !Array.isArray(patchValue)) {
      const currentValue = current?.[field];
      next[field] = {
        ...(typeof currentValue === "object" && currentValue !== null ? currentValue : {}),
        ...patchValue,
      };
    }
  }
  return next;
}

/**
 * Apply a patch to a config snapshot the way the daemon will.
 *
 * The result is an optimistic view only: it exists so a control reflects the user's action
 * immediately instead of snapping back until the round trip completes. Field handling mirrors
 * `FileDaemonConfigStore::patch` in `crates/persistence/src/storage/daemon_config.rs`, so the
 * optimistic value matches the one the daemon will publish; a field the daemon ignores must not
 * appear here, or the UI would show a change that never persists.
 */
export function applyDaemonConfigPatch(
  config: MutableDaemonConfig,
  patch: MutableDaemonConfigPatch,
): MutableDaemonConfig {
  const next = { ...config };
  if (patch.relay) next.relay = mergeField(next.relay, patch.relay);
  if (patch.mcp) next.mcp = mergeField(next.mcp, patch.mcp);
  if (patch.browserTools) next.browserTools = mergeField(next.browserTools, patch.browserTools);
  if (patch.providers) {
    const providers = { ...next.providers };
    for (const [id, entry] of Object.entries(patch.providers)) {
      providers[id] = mergeProviderEntry(providers[id], entry) as (typeof providers)[string];
    }
    next.providers = providers;
  }
  if (patch.metadataGeneration) {
    const current = next.metadataGeneration;
    next.metadataGeneration = {
      ...current,
      ...patch.metadataGeneration,
      providers: patch.metadataGeneration.providers ?? current.providers,
    };
  }
  if (patch.autoArchiveAfterMerge !== undefined) {
    next.autoArchiveAfterMerge = patch.autoArchiveAfterMerge;
  }
  if (patch.enableTerminalAgentHooks !== undefined) {
    next.enableTerminalAgentHooks = patch.enableTerminalAgentHooks;
  }
  if (patch.appendSystemPrompt !== undefined) next.appendSystemPrompt = patch.appendSystemPrompt;
  if (patch.terminalProfiles) next.terminalProfiles = patch.terminalProfiles;
  if (patch.agentProfiles) next.agentProfiles = patch.agentProfiles;

  const removed = patch.removeProviders;
  if (removed && removed.length > 0) {
    const drop = new Set(removed);
    next.providers = Object.fromEntries(
      Object.entries(next.providers).filter(([id]) => !drop.has(id)),
    );
    next.metadataGeneration = {
      ...next.metadataGeneration,
      providers: next.metadataGeneration.providers.filter((entry) => !drop.has(entry.provider)),
    };
  }
  return next;
}

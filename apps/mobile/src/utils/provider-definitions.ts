import type { ProviderSnapshotEntry } from "@ait/protocol/agent-types";
import {
  AGENT_PROVIDER_DEFINITIONS,
  type AgentModeColorTier,
  type AgentModeIcon,
  type AgentProviderDefinition,
  type AgentProviderModeDefinition,
} from "@ait/protocol/provider-manifest";

function buildProviderModes(entry: ProviderSnapshotEntry): AgentProviderModeDefinition[] {
  const entryModes = entry.modes ?? [];
  const builtin = AGENT_PROVIDER_DEFINITIONS.find((definition) => definition.id === entry.provider);

  return entryModes.map((mode) => {
    const knownMode = builtin?.modes.find((candidate) => candidate.id === mode.id);
    return Object.assign({}, mode, {
      icon: (mode.icon ?? knownMode?.icon ?? "ShieldCheck") as AgentModeIcon,
      colorTier: (mode.colorTier ?? knownMode?.colorTier ?? "moderate") as AgentModeColorTier,
    });
  });
}

export function buildProviderDefinitions(
  snapshotEntries: ProviderSnapshotEntry[] | undefined,
): AgentProviderDefinition[] {
  if (!snapshotEntries?.length) {
    return [];
  }

  return snapshotEntries.map((entry) => ({
    id: entry.provider,
    ...(entry.supportsRealtimeVoice !== undefined
      ? { supportsRealtimeVoice: entry.supportsRealtimeVoice }
      : {}),
    label: entry.label ?? entry.provider,
    description: entry.description ?? "",
    defaultModeId: entry.defaultModeId ?? null,
    modes: buildProviderModes(entry),
  }));
}

export function resolveProviderLabel(
  provider: string,
  snapshotEntries: ProviderSnapshotEntry[] | undefined,
): string {
  return snapshotEntries?.find((entry) => entry.provider === provider)?.label ?? provider;
}

export function resolveProviderDefinition(
  provider: string,
  snapshotEntries: ProviderSnapshotEntry[] | undefined,
): AgentProviderDefinition | undefined {
  return buildProviderDefinitions(snapshotEntries).find((definition) => definition.id === provider);
}

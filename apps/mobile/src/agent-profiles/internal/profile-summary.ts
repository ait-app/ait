import type { ProviderSnapshotEntry } from "@ait/protocol/agent-types";
import type { AgentProfile } from "@ait/protocol/messages";
import { formatAgentModeLabel, formatThinkingOptionLabel } from "@/agent-controls/labels";

export interface AgentProfileTag {
  id: string;
  label: string;
  /** The stored id is absent from this host's catalog; the label is the raw id. */
  unresolved?: boolean;
}

function findEntry(
  entries: readonly ProviderSnapshotEntry[],
  provider: string,
): ProviderSnapshotEntry | null {
  return entries.find((entry) => entry.provider === provider) ?? null;
}

/**
 * The one-line résumé of a profile in the settings list. Every label falls back
 * to the stored id: a row must stay readable on a host whose provider catalog
 * is offline, or that no longer ships the model the profile names.
 */
export function buildAgentProfileTags(input: {
  profile: AgentProfile;
  entries: readonly ProviderSnapshotEntry[] | undefined;
  formatFeatureCount: (count: number) => string;
}): AgentProfileTag[] {
  const entries = input.entries ?? [];
  const entry = findEntry(entries, input.profile.provider);
  // No catalog yet means "not loaded", not "the host dropped the value":
  // only a loaded catalog can mark a stored id unresolved.
  const catalogLoaded = input.entries !== undefined;
  const tags: AgentProfileTag[] = [
    {
      id: "provider",
      label: entry?.label ?? input.profile.provider,
      ...(catalogLoaded && !entry ? { unresolved: true } : {}),
    },
  ];

  const modelId = input.profile.model?.trim();
  if (modelId) {
    const model = entry?.models?.find((candidate) => candidate.id === modelId);
    tags.push({
      id: "model",
      label: model?.label ?? modelId,
      ...(catalogLoaded && entry && !model ? { unresolved: true } : {}),
    });
  }

  const modeId = input.profile.modeId?.trim();
  if (modeId) {
    const mode = entry?.modes?.find((candidate) => candidate.id === modeId);
    tags.push({
      id: "mode",
      label: mode ? formatAgentModeLabel(mode) : modeId,
      ...(catalogLoaded && entry && !mode ? { unresolved: true } : {}),
    });
  }

  const thinkingOptionId = input.profile.thinkingOptionId?.trim();
  if (thinkingOptionId) {
    const model = entry?.models?.find((candidate) => candidate.id === modelId);
    const offered = model?.thinkingOptions?.some((option) => option.id === thinkingOptionId);
    tags.push({
      id: "thinking",
      label: formatThinkingOptionLabel({ id: thinkingOptionId }),
      ...(catalogLoaded && model && !offered ? { unresolved: true } : {}),
    });
  }

  const featureCount = Object.keys(input.profile.featureValues ?? {}).length;
  if (featureCount > 0) {
    tags.push({ id: "features", label: input.formatFeatureCount(featureCount) });
  }

  return tags;
}

import type { BotPermissions, BotToolGroup } from "@ait/protocol/bot/types";
import type { BotSessionSupport, ProviderSnapshotEntry } from "@ait/protocol/agent-types";
import { getUnattendedModeId } from "@ait/protocol/provider-manifest";

const DEFAULT_GROUPS: BotToolGroup[] = ["observe", "agents", "stop", "schedules"];

const TIERS: Record<string, number> = { planning: 0, safe: 1, moderate: 2, dangerous: 3 };

function tierOf(colorTier: string | undefined): number {
  // Unknown or custom tiers are treated as the most permissive.
  return (colorTier ? TIERS[colorTier] : undefined) ?? 3;
}

/** Default profile: default groups and, per ready provider, its default and safer attended modes. */
export function defaultBotPermissions(providers: ProviderSnapshotEntry[]): BotPermissions {
  const modes: Record<string, string[]> = {};
  for (const entry of providers) {
    if (entry.status !== "ready" || !entry.enabled) continue;
    const cap = defaultModeCap(entry);
    if (cap.length > 0) modes[entry.provider] = cap;
  }
  return { groups: [...DEFAULT_GROUPS], modes };
}

/** Allowed modes ordered default-first; tiers planning=0, safe=1, moderate=2, dangerous=3; unattended excluded. */
export function defaultModeCap(entry: ProviderSnapshotEntry): string[] {
  const unattended = getUnattendedModeId(entry.provider);
  const attended = (entry.modes ?? []).filter((mode) => mode.id !== unattended);
  const fallback = attended[0];
  const base = attended.find((mode) => mode.id === entry.defaultModeId) ?? fallback;
  if (!base) return [];
  const ceiling = tierOf(base.colorTier);
  return [
    base.id,
    ...attended
      .filter((mode) => mode.id !== base.id && tierOf(mode.colorTier) <= ceiling)
      .map((mode) => mode.id),
  ];
}

export type BotSessionNote =
  | "instructionsInInput"
  | "noReadOnlyMode"
  | "toolsNeedApproval"
  | "noAitTools";

/** Capability notes for the Bot's own provider; empty for full support or unknown (old host). */
export function botSessionNotes(support: BotSessionSupport | undefined): BotSessionNote[] {
  if (!support) return [];
  const notes: BotSessionNote[] = [];
  if (support.instructions === "turn_prefix") notes.push("instructionsInInput");
  if (support.readOnlyMode === null) notes.push("noReadOnlyMode");
  if (!support.aitTools) notes.push("noAitTools");
  else if (!support.toolPreapproval) notes.push("toolsNeedApproval");
  return notes;
}

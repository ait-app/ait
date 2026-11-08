import type { BotPermissions, BotToolGroup } from "@ait/protocol/bot/types";
import type { BotSessionSupport, ProviderSnapshotEntry } from "@ait/protocol/agent-types";
import { getUnattendedModeId } from "@ait/protocol/provider-manifest";

const DEFAULT_GROUPS: BotToolGroup[] = ["observe", "agents", "stop", "schedules"];

/** Default profile: default groups and, per ready provider, its default attended mode. */
export function defaultBotPermissions(providers: ProviderSnapshotEntry[]): BotPermissions {
  const modes: Record<string, string[]> = {};
  for (const entry of providers) {
    if (entry.status !== "ready" || !entry.enabled) continue;
    const mode = defaultAgentMode(entry);
    if (mode) modes[entry.provider] = [mode];
  }
  return { groups: [...DEFAULT_GROUPS], modes };
}

/** The provider's default mode, or its first attended one when the default is unattended. */
export function defaultAgentMode(entry: ProviderSnapshotEntry): string | undefined {
  const unattended = getUnattendedModeId(entry.provider);
  const attended = (entry.modes ?? []).filter((mode) => mode.id !== unattended);
  return (attended.find((mode) => mode.id === entry.defaultModeId) ?? attended[0])?.id;
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

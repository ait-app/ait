import { parseModelRoute } from "@/components/provider-preset-form";
import type { SubagentRow } from "@/subagents/select";
import { resolveAgentTitleLabel } from "@/utils/agent-title-label";

export type VibeWorkerStatus = "running" | "completed" | "failed" | "canceled" | "unknown";

export interface VibeWorker {
  id: string;
  description: string;
  model: string;
  status: VibeWorkerStatus;
}

export interface VibeSnapshot {
  /** A missing or malformed list is unavailable, not an empty workload. */
  workers: VibeWorker[] | null;
}

/** Only the adapter's confirmed mode and worker registry describe Vibe work. */
export function readVibe(value: unknown): VibeSnapshot | null {
  if (!value || typeof value !== "object") return null;
  const raw = value as Record<string, unknown>;
  if (raw.enabled !== true) return null;
  if (!Array.isArray(raw.workers)) return { workers: null };
  const workers: VibeWorker[] = [];
  const ids = new Set<string>();
  for (const entry of raw.workers) {
    if (!entry || typeof entry !== "object" || typeof entry.id !== "string" || !entry.id.trim())
      return { workers: null };
    if (ids.has(entry.id)) return { workers: null };
    ids.add(entry.id);
    const model = typeof entry.model === "string" ? entry.model.trim() : "";
    workers.push({
      id: entry.id,
      description: typeof entry.description === "string" ? entry.description.trim() : "",
      model: parseModelRoute(model)?.join(" / ") ?? (model.startsWith("[") ? "" : model),
      status: ["running", "completed", "failed", "canceled"].includes(entry.status)
        ? entry.status
        : "unknown",
    });
  }
  return { workers };
}

/** A native worker id is not an Ait Agent id; resolve it within this host and parent. */
export function findVibeWorkerRow(
  workerId: string,
  rows: readonly SubagentRow[],
  serverId: string,
  parentAgentId: string,
): SubagentRow | undefined {
  return rows.find(
    (row) =>
      row.serverId === serverId &&
      (row.kind === "provider"
        ? row.parentAgentId === parentAgentId && row.id === workerId
        : row.observation?.rootAgentId === parentAgentId && row.observation.nativeId === workerId),
  );
}

/** Keep user titles and known observation gaps consistent with the existing child list. */
export function vibeWorkersForDisplay(
  vibe: VibeSnapshot,
  rows: readonly SubagentRow[],
  serverId: string,
  parentAgentId: string,
): VibeSnapshot {
  if (!vibe.workers) return vibe;
  const entries = vibe.workers.map((worker) => ({
    worker,
    row: findVibeWorkerRow(worker.id, rows, serverId, parentAgentId),
  }));
  entries.sort((left, right) =>
    left.row && right.row
      ? left.row.createdAt.getTime() - right.row.createdAt.getTime()
      : left.row
        ? -1
        : right.row
          ? 1
          : 0,
  );
  return {
    workers: entries.map(({ worker, row }) => {
      const observation = row?.kind === "paseo" ? row.observation : undefined;
      const unconfirmed =
        observation && (observation.health !== "current" || observation.status === "unknown");
      return {
        ...worker,
        description:
          row?.kind === "paseo"
            ? (resolveAgentTitleLabel(row.title) ?? worker.description)
            : worker.description,
        status: unconfirmed && worker.status === "running" ? "unknown" : worker.status,
      };
    }),
  };
}

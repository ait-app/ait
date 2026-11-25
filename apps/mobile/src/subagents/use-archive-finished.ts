import { useEffect, useMemo, useSyncExternalStore } from "react";
import { useArchiveAgent } from "@/hooks/use-archive-agent";
import { useSessionStore } from "@/stores/session-store";
import {
  createArchiveFinishedSubagents,
  type ArchiveFinishedOutcome,
  type ArchiveFinishedState,
  type ArchiveFinishedSubagents,
} from "./archive-finished";
import { useProviderSubagentStore } from "./provider-store";
import { localSubagentRows, type SubagentRow } from "./select";

export type { ArchiveFinishedStatus } from "./archive-finished";

export interface UseArchiveFinishedSubagentsInput {
  serverId: string;
  parentAgentId: string;
  rows: readonly SubagentRow[];
}

export interface ArchiveFinishedSubagentsCapability extends ArchiveFinishedState {
  archiveFinished: () => Promise<ArchiveFinishedOutcome>;
}

export function useArchiveFinishedSubagents({
  serverId,
  parentAgentId,
  rows,
}: UseArchiveFinishedSubagentsInput): ArchiveFinishedSubagentsCapability {
  const { archiveAgent } = useArchiveAgent();
  const archiveFinished = useMemo<ArchiveFinishedSubagents>(
    () =>
      createArchiveFinishedSubagents([], {
        parentAgentId,
        getManagedSubagent: (id) => useSessionStore.getState().sessions[serverId]?.agents.get(id),
        archiveManagedSubagent: (id) => archiveAgent({ serverId, agentId: id }),
        dismissProviderSubagents: (ids) => {
          useProviderSubagentStore.getState().hideFromTrack(serverId, parentAgentId, ids);
        },
      }),
    [archiveAgent, parentAgentId, serverId],
  );
  const state = useSyncExternalStore(
    archiveFinished.subscribe,
    archiveFinished.getState,
    archiveFinished.getState,
  );

  // Children on other hosts are left to their own host; this archives only the parent's.
  useEffect(() => {
    archiveFinished.setRows(localSubagentRows(rows, serverId));
  }, [archiveFinished, rows, serverId]);

  return { ...state, archiveFinished: archiveFinished.archiveFinished };
}

import { WorkspaceDiffStatPill } from "@/composer/diff-stat-pill";
import { AgentTaskList } from "@/composer/task-list";
import { ComposerTrackBar } from "@/composer/tracks";
import { useWorkspaceHasDiffStat } from "@/composer/workspace-diff-stat";
import { useIsCompactFormFactor } from "@/constants/layout";
import { useSettings } from "@/hooks/use-settings";
import { usePaneContext } from "@/panels/pane-context";
import { useSessionStore } from "@/stores/session-store";
import {
  useArchiveSubagent,
  useDetachSubagent,
  type ArchiveFinishedStatus,
  type SubagentRow,
} from "@/subagents";
import { SubagentsTrack } from "@/subagents/track";
import type { TodoEntry } from "@/types/stream";
import { navigateToAgent } from "@/utils/navigate-to-agent";
import { buildWorkspaceTabPersistenceKey, type WorkspaceTabTarget } from "@/workspace-tabs/model";
import { openPreferredWorkspaceTarget } from "@/workspace-tabs/open-beside";
import { openComposerChanges } from "@/workspace-tabs/open-supporting-view";
import { memo, useCallback, type ReactElement } from "react";
import { useSubagentOpensInSheet } from "@/subagents";
/**
 * The pane's ambient context — workspace changes, subagents, and tasks — as a row of pills above
 * the composer.
 *
 * The row shares the composer's keyboard transform and owns the space between itself and the
 * transcript. Each pill owns its action while tab placement stays behind the workspace boundary.
 */
export const AgentTracks = memo(function AgentTracks({
  serverId,
  workspaceId,
  cwd,
  subagentRows,
  tasks,
  archiveFinishedStatus,
  onArchiveFinished,
}: {
  serverId: string;
  workspaceId: string;
  agentId: string;
  cwd: string;
  subagentRows: SubagentRow[];
  tasks: TodoEntry[] | undefined;
  archiveFinishedStatus: ArchiveFinishedStatus;
  onArchiveFinished: () => void;
}): ReactElement | null {
  const { tabId } = usePaneContext();
  const hasWorkspaceDiffStat = useWorkspaceHasDiffStat(serverId, workspaceId);
  const isCompact = useIsCompactFormFactor();
  const openInSidePane = useSettings((settings) => settings.openInSidePane);
  const workspaceKey = buildWorkspaceTabPersistenceKey({ serverId, workspaceId });
  const canDetachSubagents = useSessionStore(
    (state) => state.sessions[serverId]?.serverInfo?.features?.agentDetach === true,
  );
  const archiveSubagent = useArchiveSubagent({ serverId });
  const detachSubagent = useDetachSubagent({ serverId });
  const openSubagentTarget = useCallback(
    (target: WorkspaceTabTarget) => {
      openPreferredWorkspaceTarget({
        workspaceKey,
        target,
        source: "subagents",
        preferences: openInSidePane,
        parentTabId: tabId,
      });
    },
    [openInSidePane, tabId, workspaceKey],
  );
  const handleOpenSubagent = useCallback(
    (subagentId: string) => {
      const session = useSessionStore.getState().sessions[serverId];
      const agent = session?.agents.get(subagentId) ?? session?.agentDetails.get(subagentId);
      // A subagent in another workspace lives in that workspace's tabs.
      if (agent?.workspaceId && agent.workspaceId !== workspaceId) {
        navigateToAgent({ serverId, agentId: subagentId });
        return;
      }
      openSubagentTarget({ kind: "agent", agentId: subagentId });
    },
    [openSubagentTarget, serverId, workspaceId],
  );
  const handleOpenProviderSubagent = useCallback(
    (parentAgentId: string, subagentId: string) => {
      openSubagentTarget({ kind: "provider_subagent", parentAgentId, subagentId });
    },
    [openSubagentTarget],
  );
  const opensInSheet = useSubagentOpensInSheet(workspaceKey);
  const handleOpenChanges = useCallback(() => {
    if (!workspaceKey) {
      return;
    }
    openComposerChanges({
      isCompact,
      workspaceKey,
      checkout: { serverId, cwd, isGit: true },
      preferences: openInSidePane,
    });
  }, [cwd, isCompact, openInSidePane, serverId, workspaceKey]);

  if (
    !hasWorkspaceDiffStat &&
    !hasAgentTracks({
      subagentRows,
      tasks,
      archiveFinishedStatus,
    })
  ) {
    return null;
  }

  return (
    <ComposerTrackBar>
      <AgentTaskList tasks={tasks} />
      <SubagentsTrack
        serverId={serverId}
        rows={subagentRows}
        onOpenSubagent={handleOpenSubagent}
        onOpenProviderSubagent={handleOpenProviderSubagent}
        opensInSheet={opensInSheet}
        onArchiveSubagent={archiveSubagent}
        onArchiveFinished={onArchiveFinished}
        archiveFinishedStatus={archiveFinishedStatus}
        onDetachSubagent={canDetachSubagents ? detachSubagent : undefined}
      />

      <WorkspaceDiffStatPill
        serverId={serverId}
        workspaceId={workspaceId}
        onPress={handleOpenChanges}
      />
    </ComposerTrackBar>
  );
});

export function hasAgentTracks({
  subagentRows,
  tasks,
  archiveFinishedStatus,
}: {
  subagentRows: readonly SubagentRow[];
  tasks: readonly TodoEntry[] | undefined;
  archiveFinishedStatus: ArchiveFinishedStatus;
}): boolean {
  return subagentRows.length > 0 || Boolean(tasks?.length) || archiveFinishedStatus.kind !== "idle";
}

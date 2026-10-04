import type { CheckoutPrStatusPayload } from "./pr-status";
import type { CheckoutStatusPayload } from "./checkout-status-cache";
import type { Agent } from "@/stores/session-store";

export function hasActiveWorkspaceAgents(input: {
  agents: Iterable<
    Pick<Agent, "workspaceId" | "cwd" | "archivedAt" | "status" | "turn"> & {
      pendingPermissions: readonly unknown[];
    }
  >;
  workspaceId: string | null;
  cwd: string;
}): boolean {
  for (const agent of input.agents) {
    const belongsToWorkspace =
      input.workspaceId && agent.workspaceId
        ? agent.workspaceId === input.workspaceId
        : agent.cwd === input.cwd;
    if (
      belongsToWorkspace &&
      !agent.archivedAt &&
      (agent.turn.phase === "open" ||
        agent.status === "running" ||
        agent.status === "initializing" ||
        agent.pendingPermissions.length > 0)
    ) {
      return true;
    }
  }
  return false;
}

export function deriveWorkspaceActionState(input: {
  gitStatus: CheckoutStatusPayload | null;
  prStatus: CheckoutPrStatusPayload["status"];
  prStatusKnown: boolean;
  hasActiveAgents: boolean;
}) {
  const git = input.gitStatus?.isGit ? input.gitStatus : null;
  const branch = git?.branchStatus;
  const pr = input.prStatus;
  const hasOpenPullRequest = Boolean(pr?.url && pr.state === "open" && !pr.isMerged);
  const matchesPullRequestHead = Boolean(
    branch?.headSha &&
    pr?.headSha &&
    branch.headSha === pr.headSha &&
    git?.currentBranch === pr.headRefName,
  );
  const hasNewWork = Boolean(
    git &&
    !hasOpenPullRequest &&
    (git.aheadBehind?.ahead ?? 0) > 0 &&
    (!pr || (branch?.headSha && pr.headSha && !matchesPullRequestHead)),
  );
  const remoteCounts = branch?.aheadBehind;
  const remoteInSync = Boolean(
    branch &&
    (branch.remoteRef === null || (remoteCounts?.ahead === 0 && remoteCounts.behind === 0)),
  );
  return {
    hasOpenPullRequest,
    needsPushForOpenPr: Boolean(
      hasOpenPullRequest && branch && ((remoteCounts?.ahead ?? 0) > 0 || branch.remoteRef === null),
    ),
    hasNewWork,
    branchStatusAvailable: Boolean(
      branch &&
      git?.currentBranch &&
      (!branch.remoteRef || remoteCounts) &&
      (!pr || hasOpenPullRequest || (pr.headSha && branch.headSha)),
    ),
    hasConflicts: Boolean(branch?.hasConflicts),
    remoteAheadCount: remoteCounts?.ahead ?? null,
    remoteBehindCount: remoteCounts?.behind ?? null,
    shouldPromoteArchive: Boolean(
      input.prStatusKnown &&
      git &&
      !git.error &&
      !git.isDirty &&
      !input.hasActiveAgents &&
      !branch?.hasConflicts &&
      pr?.isMerged &&
      matchesPullRequestHead &&
      remoteInSync,
    ),
  };
}

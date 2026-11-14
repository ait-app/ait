import { useState, useCallback, useMemo, type ReactElement } from "react";
import { Info } from "lucide-react-native";
import { withUnistyles } from "react-native-unistyles";
import { useTranslation } from "react-i18next";
import type { Theme } from "@/styles/theme";
import { getForgePresentation, type Forge } from "@/git/forge";
import { ForgeBrandIcon, getForgeBrandColorMapping } from "@/git/forge-icon";
import { type CheckoutGitActionStatus, useCheckoutGitActionsStore } from "@/git/actions-store";
import { type CheckoutStatusPayload, useCheckoutStatusQuery } from "@/git/use-status-query";
import { useCheckoutPrStatusQuery } from "@/git/use-pr-status-query";
import {
  buildGitActions,
  narrowPullRequestState,
  type BuildGitActionsInput,
  type GitAction,
  type GitActions,
} from "@/git/policy";
import { deriveMergeCapability } from "@/git/merge-capability";
import type { CheckoutPrMergeMethod } from "@ait/protocol/messages";
import { openExternalUrl } from "@/utils/open-external-url";
import { useToast } from "@/contexts/toast-context";
import { useSessionStore, type WorkspaceDescriptor } from "@/stores/session-store";
import {
  useActiveWorkspaceSelection,
  type ActiveWorkspaceSelection,
} from "@/stores/navigation-active-workspace-store";
import { redirectIfArchivingActiveWorkspace } from "@/utils/sidebar-workspace-archive-redirect";
import { type WorktreeArchiveWarningLabels } from "@/git/worktree-archive-warning";
import { useWorkspaceArchive } from "@/workspace/use-workspace-archive";
import { resolveWorkspaceMapKeyByIdentity } from "@/utils/workspace-identity";
import { confirmDialog } from "@/utils/confirm-dialog";
import { deriveWorkspaceActionState, hasActiveWorkspaceAgents } from "./workspace-action-state";
import { GIT_ACTION_ICONS as icons } from "./action-icons";

const forgeMutedColorMapping = (theme: Theme) => ({ color: theme.colors.foregroundMuted });
const ThemedInfo = withUnistyles(Info, (theme) => ({ color: theme.colors.foreground }));

export function useGitActionRunner(): (action: GitAction) => void {
  const toast = useToast();

  return useCallback(
    (action: GitAction) => {
      if (action.disabled) return;
      if (action.unavailableMessage) {
        toast.show(action.unavailableMessage, {
          durationMs: 3200,
          icon: <ThemedInfo size={16} />,
        });
        return;
      }
      action.handler();
    },
    [toast],
  );
}

/**
 * The leading icon for every change-request action (create/view/merge) is the
 * forge's brand mark, tinted in its brand color (the GitLab tanuki orange, etc).
 * GitHub and unknown forges have no brand color and render in a neutral tone.
 * The merge variants all share this one icon, so we build it once.
 */
function renderForgePrIcon(forge: Forge): ReactElement {
  const icon = getForgePresentation(forge).icon;
  return (
    <ForgeBrandIcon
      iconKind={icon}
      size={16}
      uniProps={getForgeBrandColorMapping(icon) ?? forgeMutedColorMapping}
    />
  );
}

function forgeVocabulary(forge: Forge): { context: "mr" | undefined } {
  return { context: getForgePresentation(forge).changeRequestContext };
}

function isActionDisabled(actionsDisabled: boolean, status: CheckoutGitActionStatus): boolean {
  return actionsDisabled || status === "pending";
}

function resolveBranchLabel(input: {
  currentBranch: string | null | undefined;
  notGit: boolean;
  notRepositoryLabel: string;
  unknownLabel: string;
}): string {
  if (input.currentBranch && input.currentBranch !== "HEAD") {
    return input.currentBranch;
  }
  if (input.notGit) {
    return input.notRepositoryLabel;
  }
  return input.unknownLabel;
}

function formatBaseRefLabel(baseRef: string | undefined, fallbackLabel: string): string {
  if (!baseRef) return fallbackLabel;
  const trimmed = baseRef.replace(/^refs\/(heads|remotes)\//, "").trim();
  return trimmed.startsWith("origin/") ? trimmed.slice("origin/".length) : trimmed;
}

interface UseGitActionsInput {
  serverId: string;
  cwd: string;
}

interface UseGitActionsResult {
  gitActions: GitActions;
  branchLabel: string;
  baseRefLabel: string;
  isGit: boolean;
}

interface UseWorkspaceScreenArchiveControllerInput {
  serverId: string;
  activeWorkspaceSelection: ActiveWorkspaceSelection | null;
  workspaceDirectory: string | null | undefined;
  branchLabel: string;
  gitStatus: CheckoutStatusPayload | null;
  t: (key: string, options?: Record<string, unknown>) => string;
}

function resolveArchiveWorkspaceDescriptor(input: {
  workspaces: Map<string, WorkspaceDescriptor> | undefined;
  activeWorkspaceSelection: ActiveWorkspaceSelection | null;
  workspaceDirectory: string | null | undefined;
}): WorkspaceDescriptor | null {
  const activeWorkspaceKey = input.activeWorkspaceSelection
    ? resolveWorkspaceMapKeyByIdentity({
        workspaces: input.workspaces,
        workspaceId: input.activeWorkspaceSelection.workspaceId,
      })
    : null;
  if (activeWorkspaceKey) {
    return input.workspaces?.get(activeWorkspaceKey) ?? null;
  }
  if (!input.workspaceDirectory) {
    return null;
  }
  for (const candidate of input.workspaces?.values() ?? []) {
    if (candidate.workspaceDirectory === input.workspaceDirectory) {
      return candidate;
    }
  }
  return null;
}

function resolveWorkspaceArchiveRisk(
  workspace: WorkspaceDescriptor | null,
  gitStatus: CheckoutStatusPayload | null,
): { isDirty: boolean | null | undefined; aheadOfOrigin: number | null | undefined } {
  return {
    isDirty: gitStatus?.isDirty ?? workspace?.gitRuntime?.isDirty,
    aheadOfOrigin: gitStatus?.aheadOfOrigin ?? workspace?.gitRuntime?.aheadOfOrigin,
  };
}

function canArchiveWorkspace(
  workspace: WorkspaceDescriptor | null,
  risk: ReturnType<typeof resolveWorkspaceArchiveRisk>,
): boolean {
  return (
    workspace !== null &&
    (workspace.workspaceKind !== "worktree" ||
      (risk.isDirty !== undefined && risk.aheadOfOrigin !== undefined))
  );
}

function useWorkspaceScreenArchiveController({
  serverId,
  activeWorkspaceSelection,
  workspaceDirectory,
  branchLabel,
  gitStatus,
  t,
}: UseWorkspaceScreenArchiveControllerInput) {
  const sessionWorkspaces = useSessionStore((state) => state.sessions[serverId]?.workspaces);
  const [isHidingWorkspace, setIsHidingWorkspace] = useState(false);
  const workspaceDescriptor = useMemo(
    () =>
      resolveArchiveWorkspaceDescriptor({
        workspaces: sessionWorkspaces,
        activeWorkspaceSelection,
        workspaceDirectory,
      }),
    [activeWorkspaceSelection, sessionWorkspaces, workspaceDirectory],
  );
  const archiveRisk = resolveWorkspaceArchiveRisk(workspaceDescriptor, gitStatus);

  const controller = useWorkspaceArchive({
    serverId,
    workspaceId: workspaceDescriptor?.id ?? "",
    workspaceKind: workspaceDescriptor?.workspaceKind ?? "directory",
    name: workspaceDescriptor?.name ?? branchLabel,
    isDirty: archiveRisk.isDirty,
    aheadOfOrigin: archiveRisk.aheadOfOrigin,
    diffStat: workspaceDescriptor?.diffStat ?? null,
    warningLabels: getWorktreeArchiveWarningLabels(t),
    onSetHiding: setIsHidingWorkspace,
    onArchiveStarted: () => {
      if (!activeWorkspaceSelection) {
        return;
      }
      redirectIfArchivingActiveWorkspace({
        serverId,
        workspaceId: activeWorkspaceSelection.workspaceId,
        activeWorkspaceSelection,
      });
    },
  });

  return {
    ...controller,
    isArchiving: workspaceDescriptor?.archivingAt != null || isHidingWorkspace,
    canArchive: canArchiveWorkspace(workspaceDescriptor, archiveRisk),
  };
}

export function useGitActions({ serverId, cwd }: UseGitActionsInput): UseGitActionsResult {
  const { t } = useTranslation();
  const toast = useToast();
  const activeWorkspaceSelection = useActiveWorkspaceSelection();
  const sessionWorkspaces = useSessionStore((state) => state.sessions[serverId]?.workspaces);
  const resetWorkspaceDescriptor = useMemo(
    () =>
      resolveArchiveWorkspaceDescriptor({
        workspaces: sessionWorkspaces,
        activeWorkspaceSelection,
        workspaceDirectory: cwd,
      }),
    [activeWorkspaceSelection, cwd, sessionWorkspaces],
  );
  const initialBranch = resetWorkspaceDescriptor?.initialBranch ?? null;
  const resetWorkspaceId = resetWorkspaceDescriptor?.id ?? null;

  const { status, isLoading: isStatusLoading } = useCheckoutStatusQuery({ serverId, cwd });
  const gitStatus = status && status.isGit ? status : null;
  const isGit = Boolean(gitStatus);
  const notGit = status !== null && !status.isGit && !status.error;
  const baseRef = gitStatus?.baseRef ?? undefined;

  const hasUncommittedChanges = Boolean(gitStatus?.isDirty);

  const {
    status: prStatus,
    githubFeaturesEnabled,
    forge,
    hasData: hasPrStatusData,
    isFetching: isPrStatusFetching,
    isError: isPrStatusError,
    payloadError: prStatusError,
  } = useCheckoutPrStatusQuery({
    serverId,
    cwd,
    enabled: isGit,
  });
  const prIcon = useMemo(() => renderForgePrIcon(forge), [forge]);
  const baseRefLabel = useMemo(
    () => formatBaseRefLabel(baseRef, t("workspace.git.diff.base")),
    [baseRef, t],
  );
  const branchLabel = resolveBranchLabel({
    currentBranch: gitStatus?.currentBranch,
    notGit,
    notRepositoryLabel: t("workspace.git.diff.notRepository"),
    unknownLabel: t("workspace.git.diff.branchUnknown"),
  });

  const commitStatus = useCheckoutGitActionsStore((s) =>
    s.getStatus({ serverId, cwd, actionId: "commit" }),
  );
  const pullStatus = useCheckoutGitActionsStore((s) =>
    s.getStatus({ serverId, cwd, actionId: "pull" }),
  );
  const pushStatus = useCheckoutGitActionsStore((s) =>
    s.getStatus({ serverId, cwd, actionId: "push" }),
  );
  const pullAndPushStatus = useCheckoutGitActionsStore((s) =>
    s.getStatus({ serverId, cwd, actionId: "pull-and-push" }),
  );
  const prCreateStatus = useCheckoutGitActionsStore((s) =>
    s.getStatus({ serverId, cwd, actionId: "create-pr" }),
  );
  const mergePrStatuses: Record<CheckoutPrMergeMethod, CheckoutGitActionStatus> = {
    squash: useCheckoutGitActionsStore((s) =>
      s.getStatus({ serverId, cwd, actionId: "merge-pr-squash" }),
    ),
    merge: useCheckoutGitActionsStore((s) =>
      s.getStatus({ serverId, cwd, actionId: "merge-pr-merge" }),
    ),
    rebase: useCheckoutGitActionsStore((s) =>
      s.getStatus({ serverId, cwd, actionId: "merge-pr-rebase" }),
    ),
  };
  const enablePrAutoMergeStatuses: Record<CheckoutPrMergeMethod, CheckoutGitActionStatus> = {
    squash: useCheckoutGitActionsStore((s) =>
      s.getStatus({ serverId, cwd, actionId: "enable-pr-auto-merge-squash" }),
    ),
    merge: useCheckoutGitActionsStore((s) =>
      s.getStatus({ serverId, cwd, actionId: "enable-pr-auto-merge-merge" }),
    ),
    rebase: useCheckoutGitActionsStore((s) =>
      s.getStatus({ serverId, cwd, actionId: "enable-pr-auto-merge-rebase" }),
    ),
  };
  const disablePrAutoMergeStatus = useCheckoutGitActionsStore((s) =>
    s.getStatus({ serverId, cwd, actionId: "disable-pr-auto-merge" }),
  );
  const mergeStatus = useCheckoutGitActionsStore((s) =>
    s.getStatus({ serverId, cwd, actionId: "merge-branch" }),
  );
  const resetWorkspaceStatus = useCheckoutGitActionsStore((s) =>
    s.getStatus({ serverId, cwd, actionId: "reset-workspace" }),
  );

  const runCommit = useCheckoutGitActionsStore((s) => s.commit);
  const runPull = useCheckoutGitActionsStore((s) => s.pull);
  const runPush = useCheckoutGitActionsStore((s) => s.push);
  const runPullAndPush = useCheckoutGitActionsStore((s) => s.pullAndPush);
  const runCreatePr = useCheckoutGitActionsStore((s) => s.createPr);
  const runMergePr = useCheckoutGitActionsStore((s) => s.mergePr);
  const runEnablePrAutoMerge = useCheckoutGitActionsStore((s) => s.enablePrAutoMerge);
  const runDisablePrAutoMerge = useCheckoutGitActionsStore((s) => s.disablePrAutoMerge);
  const runMergeBranch = useCheckoutGitActionsStore((s) => s.mergeBranch);
  const runResetWorkspace = useCheckoutGitActionsStore((s) => s.resetWorkspace);
  const githubAutoMergeActionsEnabled = useSessionStore(
    (s) =>
      s.sessions[serverId]?.serverInfo?.features?.checkoutForgeSetAutoMerge === true ||
      s.sessions[serverId]?.serverInfo?.features?.checkoutGithubSetAutoMerge === true,
  );

  const toastActionError = useCallback(
    (error: unknown, fallback: string) => {
      const message = error instanceof Error ? error.message : fallback;
      toast.error(message);
    },
    [toast],
  );

  const toastActionSuccess = useCallback(
    (message: string) => {
      toast.show(message, { variant: "success" });
    },
    [toast],
  );

  // Runs a store action and reports its outcome; labels are translated when the action settles.
  const runWithToasts = useCallback(
    (
      action: Promise<unknown>,
      successKey: string,
      failureKey: string,
      successOptions?: Record<string, unknown>,
    ) => {
      void action
        .then(() => {
          toastActionSuccess(t(successKey, successOptions));
          return;
        })
        .catch((err) => {
          toastActionError(err, t(failureKey));
        });
    },
    [t, toastActionError, toastActionSuccess],
  );

  // Handlers
  const handleCommit = useCallback(() => {
    runWithToasts(
      runCommit({ serverId, cwd }),
      "workspace.git.actions.commit.success",
      "workspace.git.actions.toasts.failedCommit",
    );
  }, [cwd, runCommit, runWithToasts, serverId]);

  const handlePull = useCallback(() => {
    runWithToasts(
      runPull({ serverId, cwd }),
      "workspace.git.actions.pull.success",
      "workspace.git.actions.toasts.failedPull",
    );
  }, [cwd, runPull, runWithToasts, serverId]);

  const handlePullAndPush = useCallback(() => {
    runWithToasts(
      runPullAndPush({ serverId, cwd }),
      "workspace.git.actions.pullAndPush.success",
      "workspace.git.actions.toasts.failedPullAndPush",
    );
  }, [cwd, runPullAndPush, runWithToasts, serverId]);

  const handleCreatePr = useCallback(() => {
    runWithToasts(
      runCreatePr({ serverId, cwd }),
      "workspace.git.actions.createPr.success",
      "workspace.git.actions.toasts.failedCreatePr",
      forgeVocabulary(forge),
    );
  }, [cwd, forge, runCreatePr, runWithToasts, serverId]);

  const handleMergePr = useCallback(
    (method: CheckoutPrMergeMethod) => {
      runWithToasts(
        runMergePr({ serverId, cwd, method }),
        "workspace.git.actions.mergePr.success",
        "workspace.git.actions.toasts.failedMergePr",
        forgeVocabulary(forge),
      );
    },
    [cwd, forge, runMergePr, runWithToasts, serverId],
  );

  const handleEnablePrAutoMerge = useCallback(
    (method: CheckoutPrMergeMethod) => {
      runWithToasts(
        runEnablePrAutoMerge({ serverId, cwd, method }),
        "workspace.git.actions.autoMerge.enabled",
        "workspace.git.actions.toasts.failedEnableAutoMerge",
      );
    },
    [cwd, runEnablePrAutoMerge, runWithToasts, serverId],
  );

  const handleDisablePrAutoMerge = useCallback(() => {
    runWithToasts(
      runDisablePrAutoMerge({ serverId, cwd }),
      "workspace.git.actions.autoMerge.disabled",
      "workspace.git.actions.toasts.failedDisableAutoMerge",
    );
  }, [cwd, runDisablePrAutoMerge, runWithToasts, serverId]);

  const handleMergeBranch = useCallback(() => {
    if (!baseRef) {
      toast.error(t("workspace.git.actions.toasts.baseRefUnavailable"));
      return;
    }
    runWithToasts(
      runMergeBranch({ serverId, cwd, baseRef }),
      "workspace.git.actions.mergeBranch.success",
      "workspace.git.actions.toasts.failedMerge",
    );
  }, [baseRef, cwd, runMergeBranch, runWithToasts, serverId, t, toast]);

  const handleResetWorkspace = useCallback(() => {
    if (!initialBranch || !resetWorkspaceId) {
      toast.error(t("workspace.git.actions.unavailable.resetNoInitialBranch"));
      return;
    }
    void confirmDialog({
      title: t("workspace.git.actions.resetWorkspace.confirmTitle"),
      message: t("workspace.git.actions.resetWorkspace.confirmMessage", { branch: initialBranch }),
      confirmLabel: t("workspace.git.actions.resetWorkspace.confirm"),
      cancelLabel: t("common.actions.cancel"),
      destructive: true,
    })
      .then(async (confirmed) => {
        if (!confirmed) return;
        await runResetWorkspace({ serverId, cwd, workspaceId: resetWorkspaceId, initialBranch });
        toastActionSuccess(t("workspace.git.actions.resetWorkspace.success"));
      })
      .catch((err) => {
        toastActionError(err, t("workspace.git.actions.toasts.failedResetWorkspace"));
      });
  }, [
    cwd,
    initialBranch,
    resetWorkspaceId,
    runResetWorkspace,
    serverId,
    t,
    toast,
    toastActionError,
    toastActionSuccess,
  ]);

  const archiveController = useWorkspaceScreenArchiveController({
    serverId,
    activeWorkspaceSelection,
    workspaceDirectory: status?.cwd,
    branchLabel,
    gitStatus,
    t,
  });

  const handleArchiveWorkspace = useCallback(() => {
    archiveController.archive();
  }, [archiveController]);

  const hasActiveAgents = useSessionStore((state) => {
    const session = state.sessions[serverId];
    if (!session?.hasHydratedAgents || !resetWorkspaceDescriptor) return true;
    return hasActiveWorkspaceAgents({
      agents: session.agents.values(),
      workspaceId: resetWorkspaceId,
      cwd,
    });
  });
  const prStatusKnown =
    hasPrStatusData && !isPrStatusFetching && !isPrStatusError && !prStatusError;
  const workspaceActionState = deriveWorkspaceActionState({
    gitStatus,
    prStatus,
    prStatusKnown,
    hasActiveAgents,
  });
  const {
    hasOpenPullRequest: hasPullRequest,
    hasNewWork,
    needsPushForOpenPr,
    shouldPromoteArchive,
    branchStatusAvailable,
    hasConflicts,
    remoteAheadCount,
    remoteBehindCount,
  } = workspaceActionState;
  const actionsDisabled = !isGit || Boolean(status?.error) || isStatusLoading;
  const aheadCount = gitStatus?.aheadBehind?.ahead ?? 0;
  const behindBaseCount = gitStatus?.aheadBehind?.behind ?? 0;
  const aheadOfOrigin = gitStatus?.aheadOfOrigin ?? null;
  const behindOfOrigin = gitStatus?.behindOfOrigin ?? null;
  const hasRemote = gitStatus?.hasRemote ?? false;
  const isPaseoOwnedWorktree = gitStatus?.isPaseoOwnedWorktree ?? false;
  const isOnBaseBranch = gitStatus?.currentBranch === baseRefLabel;
  const prActionStatus = hasPullRequest ? "idle" : prCreateStatus;
  const handlePush = useCallback(() => {
    runWithToasts(
      runPush({ serverId, cwd }),
      "workspace.git.actions.push.success",
      "workspace.git.actions.toasts.failedPush",
    );
  }, [cwd, runPush, runWithToasts, serverId]);
  const handlePrAction = useCallback(() => {
    if (!hasPullRequest) {
      handleCreatePr();
      return;
    }
    if (prStatus?.url) void openExternalUrl(prStatus.url);
  }, [handleCreatePr, hasPullRequest, prStatus?.url]);

  // Build actions
  const gitActionsInput = useMemo<BuildGitActionsInput>(() => {
    const presentation = getForgePresentation(forge);
    return {
      isGit,
      githubFeaturesEnabled,
      forgeBrandLabel: presentation.brandLabel,
      forgeChangeRequestNoun: presentation.changeRequestAbbrev,
      forgeChangeRequestContext: presentation.changeRequestContext,
      githubAutoMergeActionsEnabled,
      hasPullRequest,
      pullRequestUrl: prStatus?.url ?? null,
      pullRequestState: narrowPullRequestState(prStatus?.state),
      pullRequestIsDraft: prStatus?.isDraft ?? false,
      pullRequestIsMerged: prStatus?.isMerged ?? false,
      pullRequestMergeable: prStatus?.mergeable ?? "UNKNOWN",
      mergeCapability: deriveMergeCapability(prStatus?.forgeSpecific, prStatus?.github),
      hasRemote,
      isPaseoOwnedWorktree,
      initialBranchAvailable: Boolean(initialBranch),
      isOnBaseBranch,
      hasUncommittedChanges,
      baseRefAvailable: Boolean(baseRef),
      baseRefLabel,
      aheadCount,
      behindBaseCount,
      aheadOfOrigin,
      behindOfOrigin,
      shouldPromoteArchive,
      branchStatusAvailable,
      hasConflicts,
      hasActiveAgents,
      hasNewWork,
      needsPushForOpenPr,
      remoteAheadCount,
      remoteBehindCount,
      runtime: {
        commit: {
          disabled: isActionDisabled(actionsDisabled, commitStatus),
          status: commitStatus,
          icon: icons.commit,
          handler: handleCommit,
        },
        pull: {
          disabled: isActionDisabled(actionsDisabled, pullStatus),
          status: pullStatus,
          icon: icons.pull,
          handler: handlePull,
        },
        push: {
          disabled: isActionDisabled(actionsDisabled, pushStatus),
          status: pushStatus,
          icon: icons.push,
          handler: handlePush,
        },
        "pull-and-push": {
          disabled: isActionDisabled(actionsDisabled, pullAndPushStatus),
          status: pullAndPushStatus,
          icon: icons.pullAndPush,
          handler: handlePullAndPush,
        },
        pr: {
          disabled: isActionDisabled(actionsDisabled || !prStatusKnown, prActionStatus),
          status: prActionStatus,
          icon: prIcon,
          handler: handlePrAction,
        },
        "merge-pr-squash": {
          disabled: isActionDisabled(actionsDisabled, mergePrStatuses.squash),
          status: mergePrStatuses.squash,
          icon: prIcon,
          handler: () => handleMergePr("squash"),
        },
        "merge-pr-merge": {
          disabled: isActionDisabled(actionsDisabled, mergePrStatuses.merge),
          status: mergePrStatuses.merge,
          icon: prIcon,
          handler: () => handleMergePr("merge"),
        },
        "merge-pr-rebase": {
          disabled: isActionDisabled(actionsDisabled, mergePrStatuses.rebase),
          status: mergePrStatuses.rebase,
          icon: prIcon,
          handler: () => handleMergePr("rebase"),
        },
        "enable-pr-auto-merge-squash": {
          disabled: isActionDisabled(actionsDisabled, enablePrAutoMergeStatuses.squash),
          status: enablePrAutoMergeStatuses.squash,
          icon: prIcon,
          handler: () => handleEnablePrAutoMerge("squash"),
        },
        "enable-pr-auto-merge-merge": {
          disabled: isActionDisabled(actionsDisabled, enablePrAutoMergeStatuses.merge),
          status: enablePrAutoMergeStatuses.merge,
          icon: prIcon,
          handler: () => handleEnablePrAutoMerge("merge"),
        },
        "enable-pr-auto-merge-rebase": {
          disabled: isActionDisabled(actionsDisabled, enablePrAutoMergeStatuses.rebase),
          status: enablePrAutoMergeStatuses.rebase,
          icon: prIcon,
          handler: () => handleEnablePrAutoMerge("rebase"),
        },
        "disable-pr-auto-merge": {
          disabled: isActionDisabled(actionsDisabled, disablePrAutoMergeStatus),
          status: disablePrAutoMergeStatus,
          icon: prIcon,
          handler: handleDisablePrAutoMerge,
        },
        "merge-branch": {
          disabled: isActionDisabled(actionsDisabled, mergeStatus),
          status: mergeStatus,
          icon: icons.merge,
          handler: handleMergeBranch,
        },
        "reset-workspace": {
          disabled: isActionDisabled(actionsDisabled, resetWorkspaceStatus),
          status: resetWorkspaceStatus,
          icon: icons.resetWorkspace,
          handler: handleResetWorkspace,
        },
        "archive-workspace": {
          disabled: !archiveController.canArchive || archiveController.isArchiving,
          status: archiveController.isArchiving ? "pending" : "idle",
          icon: icons.archive,
          handler: handleArchiveWorkspace,
        },
      },
    };
  }, [
    isGit,
    hasRemote,
    hasPullRequest,
    prStatus?.url,
    prStatus?.state,
    prStatus?.isDraft,
    prStatus?.isMerged,
    prStatus?.mergeable,
    prStatus?.forgeSpecific,
    prStatus?.github,
    aheadCount,
    behindBaseCount,
    isPaseoOwnedWorktree,
    initialBranch,
    isOnBaseBranch,
    githubFeaturesEnabled,
    forge,
    githubAutoMergeActionsEnabled,
    hasUncommittedChanges,
    aheadOfOrigin,
    behindOfOrigin,
    branchStatusAvailable,
    hasConflicts,
    hasActiveAgents,
    hasNewWork,
    needsPushForOpenPr,
    remoteAheadCount,
    remoteBehindCount,
    prStatusKnown,
    prActionStatus,
    baseRefLabel,
    shouldPromoteArchive,
    actionsDisabled,
    commitStatus,
    pullStatus,
    pushStatus,
    pullAndPushStatus,
    mergePrStatuses.squash,
    mergePrStatuses.merge,
    mergePrStatuses.rebase,
    enablePrAutoMergeStatuses.squash,
    enablePrAutoMergeStatuses.merge,
    enablePrAutoMergeStatuses.rebase,
    disablePrAutoMergeStatus,
    mergeStatus,
    resetWorkspaceStatus,
    archiveController.canArchive,
    archiveController.isArchiving,
    handleCommit,
    handlePull,
    handlePush,
    handlePullAndPush,
    handlePrAction,
    handleMergePr,
    handleEnablePrAutoMerge,
    handleDisablePrAutoMerge,
    handleMergeBranch,
    handleResetWorkspace,
    handleArchiveWorkspace,
    prIcon,
    baseRef,
  ]);

  const gitActions: GitActions = useMemo(
    () => buildGitActions(gitActionsInput),
    // `t` changes with the language; the policy translates through the shared i18n instance.
    [gitActionsInput, t],
  );

  return { gitActions, branchLabel, baseRefLabel, isGit };
}

function getWorktreeArchiveWarningLabels(
  t: (key: string, options?: Record<string, unknown>) => string,
): WorktreeArchiveWarningLabels {
  return {
    title: (workspaceName) => t("workspace.git.actions.archiveWarning.title", { workspaceName }),
    confirm: t("workspace.git.actions.archiveWarning.confirm"),
    cancel: t("workspace.git.actions.archiveWarning.cancel"),
    uncommittedChanges: t("workspace.git.actions.archiveWarning.uncommittedChanges"),
    uncommittedChangesWithDiff: (diffStat) =>
      t("workspace.git.actions.archiveWarning.uncommittedChangesWithDiff", { diffStat }),
    addedLine: (count) =>
      t(
        count === 1
          ? "workspace.git.actions.archiveWarning.addedLine"
          : "workspace.git.actions.archiveWarning.addedLines",
        { count },
      ),
    deletedLine: (count) =>
      t(
        count === 1
          ? "workspace.git.actions.archiveWarning.deletedLine"
          : "workspace.git.actions.archiveWarning.deletedLines",
        { count },
      ),
    unpushedCommit: (count) =>
      t(
        count === 1
          ? "workspace.git.actions.archiveWarning.unpushedCommit"
          : "workspace.git.actions.archiveWarning.unpushedCommits",
        { count },
      ),
  };
}

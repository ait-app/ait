import { useCallback, useMemo, type ReactElement } from "react";
import { Info } from "lucide-react-native";
import { withUnistyles } from "react-native-unistyles";
import { useTranslation } from "react-i18next";
import { mutedIconColorMapping } from "@/components/ui/icon-color";
import { getForgePresentation, type Forge } from "@/git/forge";
import { ForgeBrandIcon, getForgeBrandColorMapping } from "@/git/forge-icon";
import { type CheckoutGitActionStatus, useCheckoutGitActionsStore } from "@/git/actions-store";
import { useCheckoutStatusQuery } from "@/git/use-status-query";
import { useCheckoutPrStatusQuery } from "@/git/use-pr-status-query";
import {
  buildGitControl,
  isWorkspaceDelivered,
  narrowPullRequestState,
  type BuildGitControlInput,
  type GitControl,
  type GitControlAction,
} from "@/git/control";
import { mergeMethodKey, useMergeMethodStore } from "@/git/merge-method-store";
import { deriveMergeCapability } from "@/git/merge-capability";
import type { CheckoutPrMergeMethod } from "@ait/protocol/messages";
import { openExternalUrl } from "@/utils/open-external-url";
import { useToast } from "@/contexts/toast-context";
import { useSessionStore, type WorkspaceDescriptor } from "@/stores/session-store";
import {
  useActiveWorkspaceSelection,
  type ActiveWorkspaceSelection,
} from "@/stores/navigation-active-workspace-store";
import { resolveWorkspaceMapKeyByIdentity } from "@/utils/workspace-identity";
import { confirmDialog } from "@/utils/confirm-dialog";
import { deriveWorkspaceActionState, hasActiveWorkspaceAgents } from "./workspace-action-state";
import { GIT_ACTION_ICONS as icons } from "./action-icons";

const ThemedInfo = withUnistyles(Info, (theme) => ({ color: theme.colors.foreground }));

export function useGitActionRunner(): (action: GitControlAction) => void {
  const toast = useToast();

  return useCallback(
    (action: GitControlAction) => {
      if (action.disabled) return;
      if (action.unavailableReason) {
        toast.show(action.unavailableReason, {
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
      uniProps={getForgeBrandColorMapping(icon) ?? mutedIconColorMapping}
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
  gitControl: GitControl;
  branchLabel: string;
  baseRefLabel: string;
  isGit: boolean;
}

function resolveCheckoutWorkspaceDescriptor(input: {
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

export function useGitActions({ serverId, cwd }: UseGitActionsInput): UseGitActionsResult {
  const { t } = useTranslation();
  const toast = useToast();
  const activeWorkspaceSelection = useActiveWorkspaceSelection();
  const sessionWorkspaces = useSessionStore((state) => state.sessions[serverId]?.workspaces);
  const resetWorkspaceDescriptor = useMemo(
    () =>
      resolveCheckoutWorkspaceDescriptor({
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
  const rebaseStatus = useCheckoutGitActionsStore((s) =>
    s.getStatus({ serverId, cwd, actionId: "rebase" }),
  );
  const prCreateStatus = useCheckoutGitActionsStore((s) =>
    s.getStatus({ serverId, cwd, actionId: "create-pr" }),
  );
  const mergePrSquashStatus = useCheckoutGitActionsStore((s) =>
    s.getStatus({ serverId, cwd, actionId: "merge-pr-squash" }),
  );
  const mergePrMergeStatus = useCheckoutGitActionsStore((s) =>
    s.getStatus({ serverId, cwd, actionId: "merge-pr-merge" }),
  );
  const mergePrRebaseStatus = useCheckoutGitActionsStore((s) =>
    s.getStatus({ serverId, cwd, actionId: "merge-pr-rebase" }),
  );
  const enableAutoMergeSquashStatus = useCheckoutGitActionsStore((s) =>
    s.getStatus({ serverId, cwd, actionId: "enable-pr-auto-merge-squash" }),
  );
  const enableAutoMergeMergeStatus = useCheckoutGitActionsStore((s) =>
    s.getStatus({ serverId, cwd, actionId: "enable-pr-auto-merge-merge" }),
  );
  const enableAutoMergeRebaseStatus = useCheckoutGitActionsStore((s) =>
    s.getStatus({ serverId, cwd, actionId: "enable-pr-auto-merge-rebase" }),
  );
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
  const runRebase = useCheckoutGitActionsStore((s) => s.rebase);
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
  const rebaseSupported = useSessionStore(
    (s) => s.sessions[serverId]?.serverInfo?.features?.checkoutRebase === true,
  );
  // Remembered per host and project; without a workspace descriptor the directory stands in for the project.
  const mergeMethodStoreKey = mergeMethodKey(serverId, resetWorkspaceDescriptor?.projectId ?? cwd);
  const rememberedMergeMethod = useMergeMethodStore((s) => s.methods[mergeMethodStoreKey] ?? null);
  const setMergeMethod = useMergeMethodStore((s) => s.setMethod);
  const handleSelectMergeMethod = useCallback(
    (method: CheckoutPrMergeMethod) => setMergeMethod(mergeMethodStoreKey, method),
    [mergeMethodStoreKey, setMergeMethod],
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

  const handleRebase = useCallback(() => {
    runWithToasts(
      runRebase({ serverId, cwd }),
      "workspace.git.actions.rebase.success",
      "workspace.git.actions.toasts.failedRebase",
    );
  }, [cwd, runRebase, runWithToasts, serverId]);

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
    shouldPromoteReset,
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
  const upstreamRef = gitStatus?.upstreamRef ?? null;
  const remoteRef = gitStatus?.branchStatus?.remoteRef ?? null;
  const isPaseoOwnedWorktree = gitStatus?.isPaseoOwnedWorktree ?? false;
  const isOnBaseBranch = gitStatus?.currentBranch === baseRefLabel;
  const prActionStatus = hasPullRequest ? "idle" : prCreateStatus;
  const isDelivered = isWorkspaceDelivered({
    shouldPromoteReset,
    isPaseoOwnedWorktree,
    initialBranchAvailable: Boolean(initialBranch),
    hasRemote,
    hasActiveAgents,
    hasNewWork,
    remoteAheadCount,
    remoteBehindCount,
  });
  const handleResetWorkspace = useCallback(() => {
    if (!initialBranch || !resetWorkspaceId) {
      toast.error(t("workspace.git.actions.reason.resetNoInitialBranch"));
      return;
    }
    void confirmDialog({
      title: t("workspace.git.actions.resetWorkspace.confirmTitle"),
      // Delivered work has nothing left to lose, so the warning is short; otherwise it is in full.
      message: isDelivered
        ? t("workspace.git.actions.resetWorkspace.deliveredConfirmMessage", { base: baseRefLabel })
        : t("workspace.git.actions.resetWorkspace.confirmMessage", { branch: initialBranch }),
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
    baseRefLabel,
    cwd,
    initialBranch,
    isDelivered,
    resetWorkspaceId,
    runResetWorkspace,
    serverId,
    t,
    toast,
    toastActionError,
    toastActionSuccess,
  ]);
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

  const gitControlInput = useMemo<BuildGitControlInput>(() => {
    const presentation = getForgePresentation(forge);
    const runtime = (
      status: CheckoutGitActionStatus,
      icon: ReactElement,
      handler: () => void,
      disabled = actionsDisabled,
    ) => ({ disabled: isActionDisabled(disabled, status), status, icon, handler });
    return {
      isGit,
      githubFeaturesEnabled,
      forgeBrandLabel: presentation.brandLabel,
      forgeChangeRequestNoun: presentation.changeRequestAbbrev,
      forgeChangeRequestContext: presentation.changeRequestContext,
      githubAutoMergeActionsEnabled,
      hasPullRequest,
      pullRequestUrl: prStatus?.url ?? null,
      pullRequestNumber: prStatus?.number ?? null,
      pullRequestNumberPrefix: presentation.numberPrefix,
      pullRequestState: narrowPullRequestState(prStatus?.state),
      pullRequestIsDraft: prStatus?.isDraft ?? false,
      pullRequestIsMerged: prStatus?.isMerged ?? false,
      pullRequestMergeable: prStatus?.mergeable ?? "UNKNOWN",
      mergeCapability: deriveMergeCapability(prStatus?.forgeSpecific, prStatus?.github),
      rememberedMergeMethod,
      onSelectMergeMethod: handleSelectMergeMethod,
      hasRemote,
      isPaseoOwnedWorktree,
      initialBranchAvailable: Boolean(initialBranch),
      isOnBaseBranch,
      branchLabel,
      hasUncommittedChanges,
      baseRefAvailable: Boolean(baseRef),
      baseRefLabel,
      aheadCount,
      behindBaseCount,
      upstreamRef,
      remoteRef,
      aheadOfOrigin,
      behindOfOrigin,
      shouldPromoteReset,
      branchStatusAvailable,
      hasConflicts,
      hasActiveAgents,
      hasNewWork,
      needsPushForOpenPr,
      remoteAheadCount,
      remoteBehindCount,
      rebaseSupported,
      runtime: {
        commit: runtime(commitStatus, icons.commit, handleCommit),
        pull: runtime(pullStatus, icons.pull, handlePull),
        push: runtime(pushStatus, icons.push, handlePush),
        rebase: runtime(rebaseStatus, icons.rebase, handleRebase),
        pr: runtime(prActionStatus, prIcon, handlePrAction, actionsDisabled || !prStatusKnown),
        "merge-pr": {
          squash: runtime(mergePrSquashStatus, prIcon, () => handleMergePr("squash")),
          merge: runtime(mergePrMergeStatus, prIcon, () => handleMergePr("merge")),
          rebase: runtime(mergePrRebaseStatus, prIcon, () => handleMergePr("rebase")),
        },
        "enable-pr-auto-merge": {
          squash: runtime(enableAutoMergeSquashStatus, prIcon, () =>
            handleEnablePrAutoMerge("squash"),
          ),
          merge: runtime(enableAutoMergeMergeStatus, prIcon, () =>
            handleEnablePrAutoMerge("merge"),
          ),
          rebase: runtime(enableAutoMergeRebaseStatus, prIcon, () =>
            handleEnablePrAutoMerge("rebase"),
          ),
        },
        "disable-pr-auto-merge": runtime(
          disablePrAutoMergeStatus,
          prIcon,
          handleDisablePrAutoMerge,
        ),
        "merge-branch": runtime(mergeStatus, icons.merge, handleMergeBranch),
        "reset-workspace": runtime(
          resetWorkspaceStatus,
          icons.resetWorkspace,
          handleResetWorkspace,
        ),
      },
    };
  }, [
    isGit,
    hasRemote,
    hasPullRequest,
    prStatus?.url,
    prStatus?.number,
    prStatus?.state,
    prStatus?.isDraft,
    prStatus?.isMerged,
    prStatus?.mergeable,
    prStatus?.forgeSpecific,
    prStatus?.github,
    rememberedMergeMethod,
    handleSelectMergeMethod,
    aheadCount,
    behindBaseCount,
    isPaseoOwnedWorktree,
    initialBranch,
    isOnBaseBranch,
    branchLabel,
    githubFeaturesEnabled,
    forge,
    githubAutoMergeActionsEnabled,
    hasUncommittedChanges,
    upstreamRef,
    remoteRef,
    aheadOfOrigin,
    behindOfOrigin,
    branchStatusAvailable,
    hasConflicts,
    hasActiveAgents,
    hasNewWork,
    needsPushForOpenPr,
    remoteAheadCount,
    remoteBehindCount,
    rebaseSupported,
    prStatusKnown,
    prActionStatus,
    baseRefLabel,
    shouldPromoteReset,
    actionsDisabled,
    commitStatus,
    pullStatus,
    pushStatus,
    rebaseStatus,
    mergePrSquashStatus,
    mergePrMergeStatus,
    mergePrRebaseStatus,
    enableAutoMergeSquashStatus,
    enableAutoMergeMergeStatus,
    enableAutoMergeRebaseStatus,
    disablePrAutoMergeStatus,
    mergeStatus,
    resetWorkspaceStatus,
    handleCommit,
    handlePull,
    handlePush,
    handleRebase,
    handlePrAction,
    handleMergePr,
    handleEnablePrAutoMerge,
    handleDisablePrAutoMerge,
    handleMergeBranch,
    handleResetWorkspace,
    prIcon,
    baseRef,
  ]);

  const gitControl = useMemo(
    () => buildGitControl(gitControlInput),
    // `t` changes with the language; the model translates through the shared i18n instance.
    [gitControlInput, t],
  );

  return { gitControl, branchLabel, baseRefLabel, isGit };
}

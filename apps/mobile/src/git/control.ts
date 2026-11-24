import type { ReactElement } from "react";

import type { ActionStatus } from "@/components/ui/dropdown-menu";
import { i18n } from "@/i18n/i18next";
import type { CheckoutPrMergeMethod, PullRequestMergeable } from "@ait/protocol/messages";

import type { MergeCapability } from "./merge-capability";

export type GitControlActionId =
  | "commit"
  | "pull"
  | "push"
  | "rebase"
  | "pr"
  | "merge-pr"
  | "enable-pr-auto-merge"
  | "disable-pr-auto-merge"
  | "merge-branch"
  | "reset-workspace";

export interface GitControlAction {
  id: GitControlActionId;
  label: string;
  pendingLabel: string;
  successLabel: string;
  status: ActionStatus;
  /** Busy, or Git status unusable. */
  disabled: boolean;
  /** Why the action cannot run in this state; short, shown inline. */
  unavailableReason?: string;
  /** Fixed second line (reset only). */
  description?: string;
  /** Trailing text such as "1 incoming". */
  detail?: string;
  destructive?: boolean;
  icon?: ReactElement;
  handler: () => void;
}

export type GitControlStateId =
  | "conflicts"
  | "diverged"
  | "behind"
  | "unknown"
  | "up-to-date"
  | "idle";

export type GitControlPrimary =
  | {
      kind: "action";
      action: GitControlAction;
      label: string;
      incoming: number | null;
      outgoing: number | null;
    }
  | { kind: "state"; state: GitControlStateId; label: string; tone: "warning" | "neutral" };

export type GitControlSectionId = "changes" | "sync" | "pull-request" | "local" | "danger";

export interface GitControlSection {
  id: GitControlSectionId;
  label: string | null;
  actions: GitControlAction[];
}

export interface GitControlMergeMethod {
  label: string;
  selected: CheckoutPrMergeMethod;
  options: { method: CheckoutPrMergeMethod; label: string }[];
  onSelect: (method: CheckoutPrMergeMethod) => void;
}

export interface GitControlSummary {
  branch: string;
  base: string | null;
  facts: string[];
}

export interface GitControl {
  /** Null only when the directory is not a Git checkout. */
  primary: GitControlPrimary | null;
  /** Empty sections are omitted. */
  sections: GitControlSection[];
  /** Null unless an open PR allows more than one method; unknown capability allows all three. */
  mergeMethod: GitControlMergeMethod | null;
  summary: GitControlSummary | null;
}

export interface GitControlRuntime {
  disabled: boolean;
  status: ActionStatus;
  icon?: ReactElement;
  handler: () => void;
}

type PerMethodActionId = "merge-pr" | "enable-pr-auto-merge";

export type GitControlRuntimes = Record<
  Exclude<GitControlActionId, PerMethodActionId>,
  GitControlRuntime
> &
  Record<PerMethodActionId, Record<CheckoutPrMergeMethod, GitControlRuntime>>;

export interface BuildGitControlInput {
  isGit: boolean;
  githubFeaturesEnabled: boolean;
  /** Forge brand label (e.g. "GitHub", "GitLab") for forge-neutral reasons. */
  forgeBrandLabel: string;
  /** Short change-request noun label (e.g. "PR", "MR"). */
  forgeChangeRequestNoun: string;
  /** i18n context for keys with an `_mr` variant (`"mr"` on GitLab), so labels say PR or MR. */
  forgeChangeRequestContext: "mr" | undefined;
  githubAutoMergeActionsEnabled: boolean;
  hasPullRequest: boolean;
  pullRequestUrl: string | null;
  pullRequestNumber: number | null;
  /** The forge's prefix before a change-request number ("#" or "!"). */
  pullRequestNumberPrefix: string;
  pullRequestState: "open" | "closed" | null;
  pullRequestIsDraft: boolean;
  pullRequestIsMerged: boolean;
  pullRequestMergeable: PullRequestMergeable;
  mergeCapability: MergeCapability | null;
  rememberedMergeMethod: CheckoutPrMergeMethod | null;
  onSelectMergeMethod: (method: CheckoutPrMergeMethod) => void;
  hasRemote: boolean;
  isPaseoOwnedWorktree: boolean;
  initialBranchAvailable: boolean;
  isOnBaseBranch: boolean;
  branchLabel: string;
  hasUncommittedChanges: boolean;
  baseRefAvailable: boolean;
  baseRefLabel: string;
  aheadCount: number;
  behindBaseCount: number;
  /** The configured upstream, which Pull and Push use. */
  upstreamRef: string | null;
  /** The same-named branch on the preferred remote, which the remote counts measure. */
  remoteRef: string | null;
  aheadOfOrigin: number | null;
  behindOfOrigin: number | null;
  /** The work is delivered: merged, clean and in sync, so the next step is a fresh start. */
  shouldPromoteReset: boolean;
  branchStatusAvailable: boolean;
  hasConflicts: boolean;
  hasActiveAgents: boolean;
  hasNewWork: boolean;
  needsPushForOpenPr: boolean;
  remoteAheadCount: number | null;
  remoteBehindCount: number | null;
  /** The host advertises the rebase operation. */
  rebaseSupported: boolean;
  runtime: GitControlRuntimes;
}

/** Fallback order when no remembered method is allowed. */
const MERGE_METHODS = ["squash", "merge", "rebase"] as const satisfies CheckoutPrMergeMethod[];

const MERGE_PR_LABEL_KEYS = {
  squash: "workspace.git.actions.mergePr.squash",
  merge: "workspace.git.actions.mergePr.merge",
  rebase: "workspace.git.actions.mergePr.rebase",
} as const satisfies Record<CheckoutPrMergeMethod, string>;

const ENABLE_AUTO_MERGE_LABEL_KEYS = {
  squash: "workspace.git.actions.autoMerge.enableSquash",
  merge: "workspace.git.actions.autoMerge.enableMerge",
  rebase: "workspace.git.actions.autoMerge.enableRebase",
} as const satisfies Record<CheckoutPrMergeMethod, string>;

const MERGE_METHOD_LABEL_KEYS = {
  squash: "workspace.git.actions.mergeMethod.squash",
  merge: "workspace.git.actions.mergeMethod.merge",
  rebase: "workspace.git.actions.mergeMethod.rebase",
} as const satisfies Record<CheckoutPrMergeMethod, string>;

const STATE_LABEL_KEYS = {
  conflicts: "workspace.git.actions.state.conflicts",
  diverged: "workspace.git.actions.state.diverged",
  behind: "workspace.git.actions.state.behind",
  unknown: "workspace.git.actions.state.unknown",
  "up-to-date": "workspace.git.actions.state.upToDate",
  idle: "workspace.git.actions.state.idle",
} as const satisfies Record<GitControlStateId, string>;

const WARNING_STATES: ReadonlySet<GitControlStateId> = new Set(["conflicts", "diverged", "behind"]);

interface GitControlActions {
  commit: GitControlAction;
  pull: GitControlAction;
  push: GitControlAction;
  rebase: GitControlAction | null;
  pr: GitControlAction;
  mergePr: GitControlAction | null;
  enableAutoMerge: GitControlAction | null;
  disableAutoMerge: GitControlAction | null;
  mergeBranch: GitControlAction;
  resetWorkspace: GitControlAction;
}

export function narrowPullRequestState(state: string | null | undefined): "open" | "closed" | null {
  if (state === "open") return "open";
  if (state === "closed") return "closed";
  return null;
}

export function resolveMergeMethod(
  allowed: readonly CheckoutPrMergeMethod[],
  remembered: CheckoutPrMergeMethod | null,
): CheckoutPrMergeMethod | null {
  if (remembered && allowed.includes(remembered)) return remembered;
  return MERGE_METHODS.find((method) => allowed.includes(method)) ?? null;
}

export function buildGitControl(input: BuildGitControlInput): GitControl {
  if (!input.isGit) {
    return { primary: null, sections: [], mergeMethod: null, summary: null };
  }
  const allowedMethods = getAllowedMergeMethods(input);
  const mergeMethod = resolveMergeMethod(allowedMethods, input.rememberedMergeMethod);
  const actions = buildActions(input, mergeMethod);
  return {
    primary: buildPrimary(input, actions),
    sections: buildSections(input, actions),
    mergeMethod: buildMergeMethod(input, allowedMethods, mergeMethod),
    summary: buildSummary(input),
  };
}

/** Each action once, in section order. A main-button action is always in its section too. */
export function listGitControlActions(control: GitControl): GitControlAction[] {
  return control.sections.flatMap((section) => section.actions);
}

function buildActions(
  input: BuildGitControlInput,
  mergeMethod: CheckoutPrMergeMethod | null,
): GitControlActions {
  return {
    commit: createAction("commit", input.runtime.commit, {
      label: i18n.t("workspace.git.actions.commit.label"),
      pendingLabel: i18n.t("workspace.git.actions.commit.pending"),
      successLabel: i18n.t("workspace.git.actions.commit.success"),
    }),
    pull: createAction("pull", input.runtime.pull, {
      label: i18n.t("workspace.git.actions.pull.label"),
      pendingLabel: i18n.t("workspace.git.actions.pull.pending"),
      successLabel: i18n.t("workspace.git.actions.pull.success"),
      unavailableReason: getPullReason(input),
      detail:
        (input.behindOfOrigin ?? 0) > 0
          ? i18n.t("workspace.git.actions.detail.incoming", { count: input.behindOfOrigin })
          : undefined,
    }),
    push: createAction("push", input.runtime.push, {
      label: i18n.t("workspace.git.actions.push.label"),
      pendingLabel: i18n.t("workspace.git.actions.push.pending"),
      successLabel: i18n.t("workspace.git.actions.push.success"),
      unavailableReason: getPushReason(input),
      detail:
        (input.aheadOfOrigin ?? 0) > 0
          ? i18n.t("workspace.git.actions.detail.outgoing", { count: input.aheadOfOrigin })
          : undefined,
    }),
    rebase: input.rebaseSupported
      ? createAction("rebase", input.runtime.rebase, {
          label: i18n.t("workspace.git.actions.rebase.label"),
          pendingLabel: i18n.t("workspace.git.actions.rebase.pending"),
          successLabel: i18n.t("workspace.git.actions.rebase.success"),
          unavailableReason: getRebaseReason(input),
        })
      : null,
    pr: buildPrAction(input),
    mergePr: hasOpenPullRequest(input) ? buildMergePrAction(input, mergeMethod) : null,
    enableAutoMerge:
      mergeMethod && canEnableAutoMerge(input)
        ? createAction("enable-pr-auto-merge", input.runtime["enable-pr-auto-merge"][mergeMethod], {
            label: i18n.t(ENABLE_AUTO_MERGE_LABEL_KEYS[mergeMethod]),
            pendingLabel: i18n.t("workspace.git.actions.autoMerge.enabling"),
            successLabel: i18n.t("workspace.git.actions.autoMerge.enabled"),
          })
        : null,
    disableAutoMerge: shouldShowDisableAutoMerge(input)
      ? createAction("disable-pr-auto-merge", input.runtime["disable-pr-auto-merge"], {
          label: i18n.t("workspace.git.actions.autoMerge.disable"),
          pendingLabel: i18n.t("workspace.git.actions.autoMerge.disabling"),
          successLabel: i18n.t("workspace.git.actions.autoMerge.disabled"),
          unavailableReason:
            input.mergeCapability?.canDisableAutoMerge === true
              ? undefined
              : i18n.t("workspace.git.actions.reason.autoMergeCannotDisable"),
        })
      : null,
    mergeBranch: createAction("merge-branch", input.runtime["merge-branch"], {
      label: i18n.t("workspace.git.actions.mergeBranch.labelInto", { base: input.baseRefLabel }),
      pendingLabel: i18n.t("workspace.git.actions.mergeBranch.pending"),
      successLabel: i18n.t("workspace.git.actions.mergeBranch.success"),
      unavailableReason: getMergeBranchReason(input),
    }),
    resetWorkspace: createAction("reset-workspace", input.runtime["reset-workspace"], {
      label: i18n.t("workspace.git.actions.resetWorkspace.label"),
      pendingLabel: i18n.t("workspace.git.actions.resetWorkspace.pending"),
      successLabel: i18n.t("workspace.git.actions.resetWorkspace.success"),
      unavailableReason: getResetReason(input),
      description: i18n.t("workspace.git.actions.resetWorkspace.description", {
        base: input.baseRefLabel,
      }),
      destructive: true,
    }),
  };
}

type ActionPresentation = Pick<GitControlAction, "label" | "pendingLabel" | "successLabel"> &
  Partial<Pick<GitControlAction, "unavailableReason" | "description" | "detail" | "destructive">>;

function createAction(
  id: GitControlActionId,
  runtime: GitControlRuntime,
  presentation: ActionPresentation,
): GitControlAction {
  const action: GitControlAction = {
    id,
    label: presentation.label,
    pendingLabel: presentation.pendingLabel,
    successLabel: presentation.successLabel,
    status: runtime.status,
    disabled: runtime.disabled,
    icon: runtime.icon,
    handler: runtime.handler,
  };
  if (presentation.unavailableReason) action.unavailableReason = presentation.unavailableReason;
  if (presentation.description) action.description = presentation.description;
  if (presentation.detail) action.detail = presentation.detail;
  if (presentation.destructive) action.destructive = true;
  return action;
}

function buildPrAction(input: BuildGitControlInput): GitControlAction {
  const vocabulary = { context: input.forgeChangeRequestContext };
  const forge = { brand: input.forgeBrandLabel, noun: input.forgeChangeRequestNoun };
  if (hasOpenPullRequest(input) && input.pullRequestUrl) {
    const label = i18n.t("workspace.git.actions.viewPr", vocabulary);
    return createAction("pr", input.runtime.pr, {
      label,
      pendingLabel: label,
      successLabel: label,
      unavailableReason: input.githubFeaturesEnabled
        ? undefined
        : i18n.t("workspace.git.actions.reason.viewPrNoForge", forge),
    });
  }
  return createAction("pr", input.runtime.pr, {
    label: i18n.t("workspace.git.actions.createPr.label", vocabulary),
    pendingLabel: i18n.t("workspace.git.actions.createPr.pending", vocabulary),
    successLabel: i18n.t("workspace.git.actions.createPr.success", vocabulary),
    unavailableReason: !input.githubFeaturesEnabled
      ? i18n.t("workspace.git.actions.reason.createPrNoForge", forge)
      : !input.hasNewWork
        ? i18n.t("workspace.git.actions.reason.createPrNoCommits")
        : undefined,
  });
}

function buildMergePrAction(
  input: BuildGitControlInput,
  mergeMethod: CheckoutPrMergeMethod | null,
): GitControlAction {
  const vocabulary = { context: input.forgeChangeRequestContext };
  // With no allowed method the row still names one, and its reason says why it cannot run.
  const method = mergeMethod ?? MERGE_METHODS[0];
  return createAction("merge-pr", input.runtime["merge-pr"][method], {
    label: i18n.t(MERGE_PR_LABEL_KEYS[method], vocabulary),
    pendingLabel: i18n.t("workspace.git.actions.mergePr.pending", vocabulary),
    successLabel: i18n.t("workspace.git.actions.mergePr.success", vocabulary),
    unavailableReason: getMergePrReason(input, mergeMethod),
  });
}

function buildPrimary(input: BuildGitControlInput, actions: GitControlActions): GitControlPrimary {
  if (
    input.hasConflicts ||
    (hasOpenPullRequest(input) &&
      !input.needsPushForOpenPr &&
      input.pullRequestMergeable === "CONFLICTING")
  ) {
    return statePrimary("conflicts");
  }
  if (input.hasUncommittedChanges) {
    return actionPrimary(actions.commit);
  }
  const remoteBehind = input.remoteBehindCount ?? 0;
  const remoteAhead = input.remoteAheadCount ?? 0;
  if (remoteBehind > 0 && remoteAhead > 0) {
    return actions.rebase && isRunnable(actions.rebase)
      ? actionPrimary(actions.rebase, {
          label: i18n.t("workspace.git.actions.rebase.remote"),
          incoming: remoteBehind,
          outgoing: remoteAhead,
        })
      : statePrimary("diverged");
  }
  if (remoteBehind > 0) {
    return canPullFromButton(input, actions.pull)
      ? actionPrimary(actions.pull, { incoming: remoteBehind })
      : statePrimary("behind");
  }
  if (!input.branchStatusAvailable) {
    return statePrimary("unknown");
  }
  const pushOutgoing = { outgoing: input.aheadOfOrigin ?? input.remoteAheadCount };
  if (hasOpenPullRequest(input)) {
    // Unpushed commits rule out merging; without a runnable push the button falls to View PR.
    if (input.needsPushForOpenPr) {
      if (isRunnable(actions.push)) return actionPrimary(actions.push, pushOutgoing);
    } else if (actions.mergePr && isRunnable(actions.mergePr)) {
      return actionPrimary(actions.mergePr);
    }
    if (input.pullRequestUrl) return actionPrimary(actions.pr);
  }
  if (!input.isOnBaseBranch && input.hasNewWork && input.githubFeaturesEnabled) {
    return actionPrimary(actions.pr);
  }
  if (input.isOnBaseBranch && isRunnable(actions.push)) {
    return actionPrimary(actions.push, pushOutgoing);
  }
  if (isWorkspaceDelivered(input)) {
    return actionPrimary(actions.resetWorkspace);
  }
  return statePrimary(
    input.remoteAheadCount === 0 && input.remoteBehindCount === 0 ? "up-to-date" : "idle",
  );
}

function actionPrimary(
  action: GitControlAction,
  overrides: { label?: string; incoming?: number | null; outgoing?: number | null } = {},
): GitControlPrimary {
  return {
    kind: "action",
    action,
    label: overrides.label ?? action.label,
    incoming: overrides.incoming ?? null,
    outgoing: overrides.outgoing ?? null,
  };
}

function statePrimary(state: GitControlStateId): GitControlPrimary {
  return {
    kind: "state",
    state,
    label: i18n.t(STATE_LABEL_KEYS[state]),
    tone: WARNING_STATES.has(state) ? "warning" : "neutral",
  };
}

function isRunnable(action: GitControlAction): boolean {
  return action.unavailableReason === undefined;
}

/** The button's count is measured against `remoteRef`, so Pull must target that same ref. */
function canPullFromButton(input: BuildGitControlInput, pull: GitControlAction): boolean {
  return isRunnable(pull) && input.remoteRef !== null && input.upstreamRef === input.remoteRef;
}

type DeliveredFacts = Pick<
  BuildGitControlInput,
  | "shouldPromoteReset"
  | "isPaseoOwnedWorktree"
  | "initialBranchAvailable"
  | "hasRemote"
  | "hasActiveAgents"
  | "hasNewWork"
  | "remoteAheadCount"
  | "remoteBehindCount"
>;

/** The work is merged, clean and in sync and the workspace can be reset: the next step is a fresh start. */
export function isWorkspaceDelivered(facts: DeliveredFacts): boolean {
  return (
    facts.shouldPromoteReset &&
    facts.isPaseoOwnedWorktree &&
    getResetReason(facts) === undefined &&
    !facts.hasActiveAgents &&
    !facts.hasNewWork &&
    (facts.remoteAheadCount ?? 0) === 0 &&
    (facts.remoteBehindCount ?? 0) === 0
  );
}

function buildSections(
  input: BuildGitControlInput,
  actions: GitControlActions,
): GitControlSection[] {
  const sections: GitControlSection[] = [
    {
      id: "changes",
      label: i18n.t("workspace.git.actions.sections.changes"),
      actions: input.hasUncommittedChanges ? [actions.commit] : [],
    },
    {
      id: "sync",
      label: i18n.t("workspace.git.actions.sections.sync"),
      actions: compact([actions.pull, actions.push, actions.rebase]),
    },
  ];
  // An open PR keeps its group on the base branch too, so its main-button action stays listed.
  if (!input.isOnBaseBranch || hasOpenPullRequest(input)) {
    sections.push({
      id: "pull-request",
      label: i18n.t("workspace.git.actions.sections.pullRequest", {
        context: input.forgeChangeRequestContext,
      }),
      actions: compact([
        actions.pr,
        actions.mergePr,
        actions.enableAutoMerge,
        actions.disableAutoMerge,
      ]),
    });
  }
  if (!input.isOnBaseBranch) {
    sections.push({
      id: "local",
      label: i18n.t("workspace.git.actions.sections.local"),
      actions: [actions.mergeBranch],
    });
  }
  if (input.isPaseoOwnedWorktree) {
    sections.push({ id: "danger", label: null, actions: [actions.resetWorkspace] });
  }
  return sections.filter((section) => section.actions.length > 0);
}

function compact(actions: (GitControlAction | null)[]): GitControlAction[] {
  return actions.filter((action): action is GitControlAction => action !== null);
}

function buildMergeMethod(
  input: BuildGitControlInput,
  allowedMethods: readonly CheckoutPrMergeMethod[],
  selected: CheckoutPrMergeMethod | null,
): GitControlMergeMethod | null {
  if (!hasOpenPullRequest(input) || allowedMethods.length < 2 || selected === null) {
    return null;
  }
  return {
    label: i18n.t("workspace.git.actions.mergeMethod.label"),
    selected,
    options: allowedMethods.map((method) => ({
      method,
      label: i18n.t(MERGE_METHOD_LABEL_KEYS[method]),
    })),
    onSelect: input.onSelectMergeMethod,
  };
}

function buildSummary(input: BuildGitControlInput): GitControlSummary {
  const facts: string[] = [];
  if (input.remoteRef === null && input.hasRemote) {
    facts.push(i18n.t("workspace.git.actions.summary.notPushed"));
  }
  if ((input.remoteAheadCount ?? 0) > 0) {
    facts.push(i18n.t("workspace.git.actions.summary.ahead", { count: input.remoteAheadCount }));
  }
  if ((input.remoteBehindCount ?? 0) > 0) {
    facts.push(i18n.t("workspace.git.actions.summary.behind", { count: input.remoteBehindCount }));
  }
  facts.push(
    input.hasUncommittedChanges
      ? i18n.t("workspace.git.actions.summary.dirty")
      : i18n.t("workspace.git.actions.summary.clean"),
  );
  if (input.hasPullRequest && input.pullRequestNumber !== null) {
    facts.push(
      i18n.t("workspace.git.actions.summary.pullRequest", {
        noun: input.forgeChangeRequestNoun,
        number: `${input.pullRequestNumberPrefix}${input.pullRequestNumber}`,
      }),
    );
  }
  return {
    branch: input.branchLabel,
    base: input.isOnBaseBranch || !input.baseRefAvailable ? null : input.baseRefLabel,
    facts,
  };
}

function hasOpenPullRequest(input: BuildGitControlInput): boolean {
  return input.hasPullRequest && input.pullRequestState === "open" && !input.pullRequestIsMerged;
}

function getAllowedMergeMethods(input: BuildGitControlInput): CheckoutPrMergeMethod[] {
  const allowed = input.mergeCapability?.allowedMethods;
  return MERGE_METHODS.filter((method) => !allowed || allowed.includes(method));
}

function hasPushableCommits(input: BuildGitControlInput): boolean {
  if ((input.aheadOfOrigin ?? 0) > 0) {
    return true;
  }
  // No-upstream managed worktrees are first-pushable: the host push sets upstream with `git push -u`.
  // Do not fold this into aheadOfOrigin; null also covers deleted/pruned upstream branches.
  return input.isPaseoOwnedWorktree && input.aheadOfOrigin === null && input.aheadCount > 0;
}

function hasBaseUpdates(input: BuildGitControlInput): boolean {
  return (
    !input.isOnBaseBranch &&
    input.baseRefAvailable &&
    !input.hasUncommittedChanges &&
    input.behindBaseCount > 0
  );
}

function getPullReason(input: BuildGitControlInput): string | undefined {
  if (!input.hasRemote) {
    return i18n.t("workspace.git.actions.reason.pullNoRemote");
  }
  if (input.hasUncommittedChanges) {
    return i18n.t("workspace.git.actions.reason.pullDirty");
  }
  if (input.behindOfOrigin === null) {
    return i18n.t("workspace.git.actions.reason.pullNoRemote");
  }
  if (input.behindOfOrigin === 0) {
    return i18n.t("workspace.git.actions.reason.pullUpToDate");
  }
  return undefined;
}

function getPushReason(input: BuildGitControlInput): string | undefined {
  if (!input.hasRemote) {
    return i18n.t("workspace.git.actions.reason.pushNoRemote");
  }
  if ((input.behindOfOrigin ?? 0) > 0) {
    // A diverged branch is fixed by the Rebase row; a pull would be refused.
    return (input.remoteAheadCount ?? 0) > 0 && canRebase(input)
      ? i18n.t("workspace.git.actions.reason.pushRebaseFirst")
      : i18n.t("workspace.git.actions.reason.pushBehind");
  }
  if (!hasPushableCommits(input)) {
    return i18n.t("workspace.git.actions.reason.pushNothing");
  }
  return undefined;
}

function canRebase(input: BuildGitControlInput): boolean {
  return input.rebaseSupported && getRebaseReason(input) === undefined;
}

/** Client mirror of the host's rebase rules; the host check is the authority. */
function getRebaseReason(input: BuildGitControlInput): string | undefined {
  if (input.remoteRef === null) {
    return i18n.t("workspace.git.actions.reason.rebaseNoRemote");
  }
  if (input.upstreamRef !== null && input.upstreamRef !== input.remoteRef) {
    return i18n.t("workspace.git.actions.reason.rebaseOtherTarget");
  }
  if (input.hasUncommittedChanges) {
    return i18n.t("workspace.git.actions.reason.rebaseDirty");
  }
  if ((input.remoteBehindCount ?? 0) === 0) {
    return i18n.t("workspace.git.actions.reason.rebaseNothing");
  }
  return undefined;
}

function getMergeBranchReason(input: BuildGitControlInput): string | undefined {
  if (!input.baseRefAvailable) {
    return i18n.t("workspace.git.actions.reason.mergeNoBase");
  }
  if (input.hasUncommittedChanges) {
    return i18n.t("workspace.git.actions.reason.mergeDirty");
  }
  if (input.aheadCount === 0) {
    return i18n.t("workspace.git.actions.reason.mergeNothing");
  }
  return undefined;
}

function getResetReason(
  input: Pick<BuildGitControlInput, "initialBranchAvailable" | "hasRemote">,
): string | undefined {
  if (!input.initialBranchAvailable) {
    return i18n.t("workspace.git.actions.reason.resetNoInitialBranch");
  }
  if (!input.hasRemote) {
    return i18n.t("workspace.git.actions.reason.resetNoRemote");
  }
  return undefined;
}

/** Why an open PR cannot be merged directly right now; undefined when it can. */
function getMergePrReason(
  input: BuildGitControlInput,
  mergeMethod: CheckoutPrMergeMethod | null,
): string | undefined {
  const forge = { brand: input.forgeBrandLabel, noun: input.forgeChangeRequestNoun };
  if (!input.githubFeaturesEnabled) {
    return i18n.t("workspace.git.actions.reason.mergePrNoForge", forge);
  }
  if (input.pullRequestIsDraft) {
    return i18n.t("workspace.git.actions.reason.mergePrDraft");
  }
  if (input.pullRequestMergeable === "CONFLICTING") {
    return i18n.t("workspace.git.actions.reason.mergePrConflicts", forge);
  }
  if (input.hasUncommittedChanges) {
    return i18n.t("workspace.git.actions.reason.mergeDirty");
  }
  if (input.aheadCount === 0) {
    return i18n.t("workspace.git.actions.reason.mergeNothing");
  }
  const capability = input.mergeCapability;
  if (capability === null) {
    const readyFromGitState =
      input.pullRequestMergeable === "MERGEABLE" &&
      input.behindOfOrigin === 0 &&
      input.aheadOfOrigin === 0 &&
      !hasBaseUpdates(input);
    return readyFromGitState
      ? undefined
      : i18n.t("workspace.git.actions.reason.mergePrNotReady", forge);
  }
  if (capability.autoMergeEnabled) {
    return i18n.t("workspace.git.actions.reason.mergePrAutoMerge");
  }
  if (capability.mergeBlockedByQueue) {
    return i18n.t("workspace.git.actions.reason.mergePrQueue");
  }
  if (!capability.directMergeReady || mergeMethod === null) {
    return i18n.t("workspace.git.actions.reason.mergePrNotReady", forge);
  }
  return undefined;
}

function canEnableAutoMerge(input: BuildGitControlInput): boolean {
  const capability = input.mergeCapability;
  return (
    input.githubFeaturesEnabled &&
    input.githubAutoMergeActionsEnabled &&
    hasOpenPullRequest(input) &&
    !input.pullRequestIsDraft &&
    input.pullRequestMergeable !== "CONFLICTING" &&
    capability !== null &&
    !capability.autoMergeEnabled &&
    capability.canEnableAutoMerge &&
    !capability.mergeBlockedByQueue
  );
}

function shouldShowDisableAutoMerge(input: BuildGitControlInput): boolean {
  return (
    hasOpenPullRequest(input) &&
    input.githubAutoMergeActionsEnabled &&
    input.mergeCapability?.autoMergeEnabled === true
  );
}

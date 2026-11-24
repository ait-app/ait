import { afterEach, describe, expect, it } from "vitest";
import type { CheckoutPrMergeMethod } from "@ait/protocol/messages";
import { i18n } from "@/i18n/i18next";

import {
  buildGitControl,
  isWorkspaceDelivered,
  listGitControlActions,
  resolveMergeMethod,
  type BuildGitControlInput,
  type GitControl,
  type GitControlAction,
  type GitControlActionId,
  type GitControlRuntime,
} from "./control";
import type { MergeCapability } from "./merge-capability";

function runtime(): GitControlRuntime {
  return { disabled: false, status: "idle", handler: () => undefined };
}

function perMethod(): Record<CheckoutPrMergeMethod, GitControlRuntime> {
  return { squash: runtime(), merge: runtime(), rebase: runtime() };
}

function capability(overrides: Partial<MergeCapability> = {}): MergeCapability {
  return {
    directMergeReady: true,
    canEnableAutoMerge: false,
    autoMergeEnabled: false,
    canDisableAutoMerge: false,
    mergeBlockedByQueue: false,
    allowedMethods: ["merge", "squash", "rebase"],
    preferredMethod: "squash",
    ...overrides,
  };
}

function createInput(overrides: Partial<BuildGitControlInput> = {}): BuildGitControlInput {
  return {
    isGit: true,
    githubFeaturesEnabled: true,
    forgeBrandLabel: "GitHub",
    forgeChangeRequestNoun: "PR",
    forgeChangeRequestContext: undefined,
    githubAutoMergeActionsEnabled: true,
    hasPullRequest: false,
    pullRequestUrl: null,
    pullRequestNumber: null,
    pullRequestNumberPrefix: "#",
    pullRequestState: null,
    pullRequestIsDraft: false,
    pullRequestIsMerged: false,
    pullRequestMergeable: "UNKNOWN",
    mergeCapability: null,
    rememberedMergeMethod: null,
    onSelectMergeMethod: () => undefined,
    hasRemote: true,
    isPaseoOwnedWorktree: false,
    initialBranchAvailable: false,
    isOnBaseBranch: true,
    branchLabel: "main",
    hasUncommittedChanges: false,
    baseRefAvailable: true,
    baseRefLabel: "main",
    aheadCount: 0,
    behindBaseCount: 0,
    upstreamRef: "origin/main",
    remoteRef: "origin/main",
    aheadOfOrigin: 0,
    behindOfOrigin: 0,
    shouldPromoteReset: false,
    branchStatusAvailable: true,
    hasConflicts: false,
    hasActiveAgents: false,
    hasNewWork: false,
    needsPushForOpenPr: false,
    remoteAheadCount: 0,
    remoteBehindCount: 0,
    rebaseSupported: true,
    runtime: {
      commit: runtime(),
      pull: runtime(),
      push: runtime(),
      rebase: runtime(),
      pr: runtime(),
      "merge-pr": perMethod(),
      "enable-pr-auto-merge": perMethod(),
      "disable-pr-auto-merge": runtime(),
      "merge-branch": runtime(),
      "reset-workspace": runtime(),
    },
    ...overrides,
  };
}

/** A feature branch in a managed worktree, pushed and in sync with its remote copy. */
function featureBranch(overrides: Partial<BuildGitControlInput> = {}): BuildGitControlInput {
  return createInput({
    isOnBaseBranch: false,
    branchLabel: "snobbish-lizard",
    isPaseoOwnedWorktree: true,
    initialBranchAvailable: true,
    upstreamRef: "origin/snobbish-lizard",
    remoteRef: "origin/snobbish-lizard",
    ...overrides,
  });
}

/** An open PR on a feature branch that everything has been pushed to. */
function openPullRequest(overrides: Partial<BuildGitControlInput> = {}): BuildGitControlInput {
  return featureBranch({
    aheadCount: 2,
    hasPullRequest: true,
    pullRequestUrl: "https://example.com/pr/812",
    pullRequestNumber: 812,
    pullRequestState: "open",
    pullRequestMergeable: "MERGEABLE",
    mergeCapability: capability(),
    ...overrides,
  });
}

function findAction(control: GitControl, id: GitControlActionId): GitControlAction | undefined {
  return listGitControlActions(control).find((action) => action.id === id);
}

describe("buildGitControl main button", () => {
  afterEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("names local conflicts and opens the panel", () => {
    const control = buildGitControl(featureBranch({ hasConflicts: true }));
    expect(control.primary).toEqual({
      kind: "state",
      state: "conflicts",
      label: "Conflicts",
      tone: "warning",
    });
  });

  it("names conflicts of an open PR with nothing to push", () => {
    const control = buildGitControl(openPullRequest({ pullRequestMergeable: "CONFLICTING" }));
    expect(control.primary).toMatchObject({
      kind: "state",
      state: "conflicts",
      label: "Conflicts",
    });
  });

  it("commits uncommitted changes", () => {
    const control = buildGitControl(featureBranch({ hasUncommittedChanges: true }));
    expect(control.primary).toMatchObject({
      kind: "action",
      label: "Commit",
      action: { id: "commit" },
      incoming: null,
      outgoing: null,
    });
  });

  it("rebases a branch diverged from its remote copy", () => {
    const control = buildGitControl(
      featureBranch({
        remoteAheadCount: 2,
        remoteBehindCount: 1,
        aheadOfOrigin: 2,
        behindOfOrigin: 1,
      }),
    );
    expect(control.primary).toMatchObject({
      kind: "action",
      label: "Rebase remote",
      action: { id: "rebase" },
      incoming: 1,
      outgoing: 2,
    });
  });

  it("pulls a branch that is only behind its remote copy", () => {
    const control = buildGitControl(featureBranch({ remoteBehindCount: 3, behindOfOrigin: 3 }));
    expect(control.primary).toMatchObject({
      kind: "action",
      label: "Pull",
      action: { id: "pull" },
      incoming: 3,
      outgoing: null,
    });
  });

  it("names an unknown Git status and opens the panel", () => {
    const control = buildGitControl(featureBranch({ branchStatusAvailable: false }));
    expect(control.primary).toEqual({
      kind: "state",
      state: "unknown",
      label: "Git",
      tone: "neutral",
    });
  });

  it("pushes commits for an open PR", () => {
    const control = buildGitControl(
      openPullRequest({ needsPushForOpenPr: true, remoteAheadCount: 2, aheadOfOrigin: 2 }),
    );
    expect(control.primary).toMatchObject({
      kind: "action",
      label: "Push",
      action: { id: "push" },
      incoming: null,
      outgoing: 2,
    });
  });

  it("merges an open PR that can be merged now", () => {
    const control = buildGitControl(openPullRequest());
    expect(control.primary).toMatchObject({
      kind: "action",
      label: "Merge PR (squash)",
      action: { id: "merge-pr" },
      incoming: null,
      outgoing: null,
    });
  });

  it("views an open PR otherwise", () => {
    const control = buildGitControl(
      openPullRequest({ mergeCapability: capability({ directMergeReady: false }) }),
    );
    expect(control.primary).toMatchObject({
      kind: "action",
      label: "View PR",
      action: { id: "pr" },
    });
  });

  it("creates a PR for new commits when the forge is connected", () => {
    const control = buildGitControl(
      featureBranch({ aheadCount: 1, hasNewWork: true, remoteRef: null, remoteAheadCount: null }),
    );
    expect(control.primary).toMatchObject({
      kind: "action",
      label: "Create PR",
      action: { id: "pr" },
    });
  });

  it("pushes the base branch's new commits", () => {
    const control = buildGitControl(
      createInput({ aheadCount: 0, aheadOfOrigin: 2, remoteAheadCount: 2 }),
    );
    expect(control.primary).toMatchObject({
      kind: "action",
      label: "Push",
      action: { id: "push" },
      outgoing: 2,
    });
  });

  it("resets a delivered workspace", () => {
    const control = buildGitControl(
      featureBranch({
        shouldPromoteReset: true,
        hasPullRequest: true,
        pullRequestState: "closed",
        pullRequestIsMerged: true,
      }),
    );
    expect(control.primary).toMatchObject({
      kind: "action",
      label: "Reset workspace",
      action: { id: "reset-workspace" },
    });
  });

  it("treats a delivered workspace as delivered only while it can be reset", () => {
    const delivered = featureBranch({ shouldPromoteReset: true });
    expect(isWorkspaceDelivered(delivered)).toBe(true);
    expect(isWorkspaceDelivered({ ...delivered, initialBranchAvailable: false })).toBe(false);
    expect(isWorkspaceDelivered({ ...delivered, hasActiveAgents: true })).toBe(false);
  });

  it("names a clean, in-sync checkout with nothing to do as up to date", () => {
    const control = buildGitControl(createInput());
    expect(control.primary).toEqual({
      kind: "state",
      state: "up-to-date",
      label: "Up to date",
      tone: "neutral",
    });
  });

  it("names a checkout with nothing to do but not in sync as Git", () => {
    const control = buildGitControl(
      featureBranch({
        githubFeaturesEnabled: false,
        aheadCount: 1,
        hasNewWork: true,
        remoteRef: null,
        remoteAheadCount: null,
        remoteBehindCount: null,
      }),
    );
    expect(control.primary).toEqual({
      kind: "state",
      state: "idle",
      label: "Git",
      tone: "neutral",
    });
  });
});

describe("buildGitControl ordering and fallbacks", () => {
  it("puts conflicts before uncommitted changes", () => {
    const control = buildGitControl(
      featureBranch({ hasConflicts: true, hasUncommittedChanges: true }),
    );
    expect(control.primary).toMatchObject({ kind: "state", state: "conflicts" });
  });

  it.each([
    { remoteAheadCount: 1, remoteBehindCount: 2, behindOfOrigin: 2, aheadOfOrigin: 1 },
    { remoteAheadCount: 0, remoteBehindCount: 2, behindOfOrigin: 2, aheadOfOrigin: 0 },
  ])("puts uncommitted changes before a remote that moved: %o", (counts) => {
    const control = buildGitControl(featureBranch({ hasUncommittedChanges: true, ...counts }));
    expect(control.primary).toMatchObject({ kind: "action", action: { id: "commit" } });
  });

  it("names a diverged branch without rebase support and offers no rebase", () => {
    const control = buildGitControl(
      featureBranch({ rebaseSupported: false, remoteAheadCount: 1, remoteBehindCount: 2 }),
    );
    expect(control.primary).toEqual({
      kind: "state",
      state: "diverged",
      label: "Diverged",
      tone: "warning",
    });
    expect(findAction(control, "rebase")).toBeUndefined();
  });

  it("names a diverged branch whose upstream is another ref and explains the rebase row", () => {
    const control = buildGitControl(
      featureBranch({
        upstreamRef: "origin/other",
        remoteAheadCount: 1,
        remoteBehindCount: 2,
      }),
    );
    expect(control.primary).toMatchObject({ kind: "state", state: "diverged" });
    const rebase = findAction(control, "rebase");
    expect(rebase).toBeDefined();
    expect(rebase?.unavailableReason).toBeTruthy();
  });

  it("names a branch behind its remote copy when the upstream is another ref", () => {
    const control = buildGitControl(
      featureBranch({ upstreamRef: "origin/other", remoteBehindCount: 2, behindOfOrigin: 2 }),
    );
    expect(control.primary).toEqual({
      kind: "state",
      state: "behind",
      label: "Behind remote",
      tone: "warning",
    });
  });

  it("names a PR status that is unknown along with the branch status", () => {
    const control = buildGitControl(
      featureBranch({
        branchStatusAvailable: false,
        hasPullRequest: false,
        pullRequestState: null,
        mergeCapability: null,
        remoteAheadCount: null,
        remoteBehindCount: null,
      }),
    );
    expect(control.primary).toEqual({
      kind: "state",
      state: "unknown",
      label: "Git",
      tone: "neutral",
    });
  });
});

describe("buildGitControl merge method", () => {
  it("merges with the remembered method when the repository allows it", () => {
    const control = buildGitControl(openPullRequest({ rememberedMergeMethod: "rebase" }));
    expect(control.primary).toMatchObject({
      kind: "action",
      label: "Merge PR (rebase)",
      action: { id: "merge-pr" },
    });
    expect(control.mergeMethod?.selected).toBe("rebase");
  });

  it("falls back to the first allowed method when the remembered one is not allowed", () => {
    const control = buildGitControl(
      openPullRequest({
        rememberedMergeMethod: "rebase",
        mergeCapability: capability({ allowedMethods: ["merge", "squash"] }),
      }),
    );
    expect(control.primary).toMatchObject({ label: "Merge PR (squash)" });
    expect(control.mergeMethod?.selected).toBe("squash");
  });

  it("runs the handler of the chosen method", () => {
    const input = openPullRequest({ rememberedMergeMethod: "merge" });
    let ran: CheckoutPrMergeMethod | null = null;
    input.runtime["merge-pr"].merge.handler = () => {
      ran = "merge";
    };
    const control = buildGitControl(input);
    findAction(control, "merge-pr")?.handler();
    expect(ran).toBe("merge");
  });

  it("offers no method choice when only one method is allowed", () => {
    const control = buildGitControl(
      openPullRequest({ mergeCapability: capability({ allowedMethods: ["merge"] }) }),
    );
    expect(control.mergeMethod).toBeNull();
    expect(control.primary).toMatchObject({ label: "Merge PR (merge)" });
  });

  it("lists exactly the allowed methods in their fixed order", () => {
    const selected: CheckoutPrMergeMethod[] = [];
    const control = buildGitControl(
      openPullRequest({
        mergeCapability: capability({ allowedMethods: ["rebase", "squash"] }),
        onSelectMergeMethod: (method) => selected.push(method),
      }),
    );
    expect(control.mergeMethod).toMatchObject({
      label: "Merge method",
      selected: "squash",
      options: [
        { method: "squash", label: "Squash" },
        { method: "rebase", label: "Rebase" },
      ],
    });
    control.mergeMethod?.onSelect("rebase");
    expect(selected).toEqual(["rebase"]);
  });

  it("resolves the remembered method only when it is allowed", () => {
    expect(resolveMergeMethod(["squash", "merge"], "merge")).toBe("merge");
    expect(resolveMergeMethod(["rebase", "merge"], "squash")).toBe("merge");
    expect(resolveMergeMethod(["rebase"], null)).toBe("rebase");
    expect(resolveMergeMethod([], "squash")).toBeNull();
  });

  it("offers every method for an open PR whose merge capability is unknown", () => {
    const mergeMethod = buildGitControl(openPullRequest({ mergeCapability: null })).mergeMethod;
    expect(mergeMethod?.options.map((option) => option.method)).toEqual([
      "squash",
      "merge",
      "rebase",
    ]);
  });

  it("offers no method while the PR status is still loading", () => {
    const loading = featureBranch({
      aheadCount: 2,
      hasPullRequest: false,
      pullRequestState: null,
      mergeCapability: null,
    });
    expect(buildGitControl(loading).mergeMethod).toBeNull();
  });

  it.each([
    ["conflicts", { pullRequestMergeable: "CONFLICTING" as const }, "PR has conflicts"],
    [
      "a merge queue",
      { mergeCapability: capability({ mergeBlockedByQueue: true }) },
      "Uses a merge queue",
    ],
    [
      "not ready",
      { mergeCapability: capability({ directMergeReady: false }) },
      "PR isn't ready to merge",
    ],
    [
      "auto-merge already on",
      { mergeCapability: capability({ autoMergeEnabled: true }) },
      "Auto-merge is already on",
    ],
    ["the forge not connected", { githubFeaturesEnabled: false }, "GitHub isn't connected"],
  ])("explains why Merge PR cannot run with %s", (_case, overrides, reason) => {
    const mergePr = findAction(buildGitControl(openPullRequest(overrides)), "merge-pr");
    expect(mergePr?.unavailableReason).toBe(reason);
  });

  it("offers auto-merge with the chosen method", () => {
    const input = openPullRequest({
      rememberedMergeMethod: "merge",
      mergeCapability: capability({ directMergeReady: false, canEnableAutoMerge: true }),
    });
    const control = buildGitControl(input);
    const enable = findAction(control, "enable-pr-auto-merge");
    expect(enable).toMatchObject({ label: "Auto merge (merge)" });
    expect(enable?.unavailableReason).toBeUndefined();
    expect(enable?.handler).toBe(input.runtime["enable-pr-auto-merge"].merge.handler);
    expect(findAction(control, "disable-pr-auto-merge")).toBeUndefined();
  });

  it("offers to disable auto-merge once it is on", () => {
    const control = buildGitControl(
      openPullRequest({
        mergeCapability: capability({ autoMergeEnabled: true, canDisableAutoMerge: true }),
      }),
    );
    const disable = findAction(control, "disable-pr-auto-merge");
    expect(disable).toMatchObject({ label: "Disable auto-merge" });
    expect(disable?.unavailableReason).toBeUndefined();
    expect(findAction(control, "enable-pr-auto-merge")).toBeUndefined();
  });

  it("explains why auto-merge cannot be disabled", () => {
    const disable = findAction(
      buildGitControl(
        openPullRequest({
          mergeCapability: capability({ autoMergeEnabled: true, canDisableAutoMerge: false }),
        }),
      ),
      "disable-pr-auto-merge",
    );
    expect(disable?.unavailableReason).toBe("You can't turn off auto-merge");
  });

  it("shows Merge PR with its reason on a draft and keeps View PR on the button", () => {
    const control = buildGitControl(openPullRequest({ pullRequestIsDraft: true }));
    expect(control.primary).toMatchObject({
      kind: "action",
      action: { id: "pr" },
      label: "View PR",
    });
    const mergePr = findAction(control, "merge-pr");
    expect(mergePr).toBeDefined();
    expect(mergePr?.unavailableReason).toBe("Still a draft");
  });
});

describe("buildGitControl panel", () => {
  function sectionIds(control: GitControl) {
    return control.sections.map((section) => section.id);
  }

  it("groups a feature branch in a managed worktree", () => {
    expect(sectionIds(buildGitControl(featureBranch()))).toEqual([
      "sync",
      "pull-request",
      "local",
      "danger",
    ]);
    expect(sectionIds(buildGitControl(featureBranch({ hasUncommittedChanges: true })))).toEqual([
      "changes",
      "sync",
      "pull-request",
      "local",
      "danger",
    ]);
  });

  it("leaves the pull request and local groups out on the base branch", () => {
    const ids = sectionIds(buildGitControl(createInput({ isPaseoOwnedWorktree: true })));
    expect(ids).not.toContain("pull-request");
    expect(ids).not.toContain("local");
  });

  it("keeps an open PR's group on the base branch but leaves the local group out", () => {
    const ids = sectionIds(buildGitControl(openPullRequest({ isOnBaseBranch: true })));
    expect(ids).toContain("pull-request");
    expect(ids).not.toContain("local");
  });

  it("leaves the danger group out of an unmanaged checkout", () => {
    const ids = sectionIds(buildGitControl(featureBranch({ isPaseoOwnedWorktree: false })));
    expect(ids).not.toContain("danger");
  });

  it("labels the groups and uses the forge's change-request noun", () => {
    const control = buildGitControl(
      featureBranch({
        hasUncommittedChanges: true,
        forgeChangeRequestNoun: "MR",
        forgeChangeRequestContext: "mr",
      }),
    );
    expect(control.sections.map((section) => section.label)).toEqual([
      "Changes",
      "Sync",
      "Merge request",
      "Local",
      null,
    ]);
  });

  it("marks reset as destructive and describes it", () => {
    const reset = findAction(buildGitControl(featureBranch()), "reset-workspace");
    expect(reset).toMatchObject({
      destructive: true,
      description: "Discard everything and return to the latest main",
    });
  });

  it("names the branch a local merge goes into", () => {
    const merge = findAction(
      buildGitControl(featureBranch({ baseRefLabel: "main" })),
      "merge-branch",
    );
    expect(merge?.label).toBe("Merge into main");
  });

  it("explains unavailable rows inline and counts sync traffic", () => {
    const upToDate = buildGitControl(featureBranch());
    expect(findAction(upToDate, "pull")?.unavailableReason).toBe("Already up to date");
    expect(findAction(upToDate, "push")?.unavailableReason).toBe("Nothing to push");
    const dirty = buildGitControl(
      featureBranch({ hasUncommittedChanges: true, behindOfOrigin: 1, remoteBehindCount: 1 }),
    );
    expect(findAction(dirty, "pull")).toMatchObject({
      unavailableReason: "Commit your changes first",
      detail: "1 incoming",
    });
    const ahead = buildGitControl(featureBranch({ aheadOfOrigin: 2, remoteAheadCount: 2 }));
    expect(findAction(ahead, "push")).toMatchObject({ detail: "2 outgoing" });
    expect(findAction(ahead, "push")?.unavailableReason).toBeUndefined();
  });

  it("offers Rebase when the branch is behind its remote copy and clean", () => {
    const rebase = findAction(
      buildGitControl(featureBranch({ remoteAheadCount: 1, remoteBehindCount: 2 })),
      "rebase",
    );
    expect(rebase).toMatchObject({ label: "Rebase", disabled: false });
    expect(rebase?.unavailableReason).toBeUndefined();
  });

  it.each([
    [
      "no remote branch",
      { upstreamRef: null, remoteRef: null, remoteAheadCount: null, remoteBehindCount: null },
      "No remote branch to rebase onto",
    ],
    [
      "a dirty tree",
      { hasUncommittedChanges: true, remoteBehindCount: 1 },
      "Commit your changes first",
    ],
    [
      "nothing to rebase",
      { remoteAheadCount: 1, remoteBehindCount: 0 },
      "Nothing new on the remote",
    ],
  ])("explains why Rebase cannot run with %s", (_case, overrides, reason) => {
    const rebase = findAction(buildGitControl(featureBranch(overrides)), "rebase");
    expect(rebase?.unavailableReason).toBe(reason);
  });

  it("points Push at the rebase on a branch diverged from its remote copy", () => {
    const push = findAction(
      buildGitControl(
        featureBranch({
          aheadCount: 1,
          aheadOfOrigin: 1,
          behindOfOrigin: 2,
          remoteAheadCount: 1,
          remoteBehindCount: 2,
        }),
      ),
      "push",
    );
    expect(push?.unavailableReason).toBe("Rebase onto the remote first");
  });

  it.each([
    ["only behind its remote copy", { behindOfOrigin: 2, remoteBehindCount: 2 }],
    [
      "diverged on a host without rebase",
      {
        aheadCount: 1,
        aheadOfOrigin: 1,
        behindOfOrigin: 2,
        remoteAheadCount: 1,
        remoteBehindCount: 2,
        rebaseSupported: false,
      },
    ],
  ])("points Push at a pull when the branch is %s", (_case, overrides) => {
    const push = findAction(buildGitControl(featureBranch(overrides)), "push");
    expect(push?.unavailableReason).toBe("Pull the newer changes first");
  });

  it("keeps busy separate from unavailable", () => {
    const input = featureBranch({ behindOfOrigin: 1, remoteBehindCount: 1 });
    input.runtime.pull = { ...input.runtime.pull, disabled: true, status: "pending" };
    const pull = findAction(buildGitControl(input), "pull");
    expect(pull).toMatchObject({ disabled: true, status: "pending" });
    expect(pull?.unavailableReason).toBeUndefined();
  });

  it("lists each action once in section order", () => {
    const control = buildGitControl(
      openPullRequest({
        hasUncommittedChanges: true,
        mergeCapability: capability({ canEnableAutoMerge: true }),
      }),
    );
    const ids = listGitControlActions(control).map((action) => action.id);
    expect(new Set(ids).size).toBe(ids.length);
    expect(ids).toEqual(
      control.sections.flatMap((section) => section.actions.map((action) => action.id)),
    );
  });

  it.each([
    ["View PR", { pullRequestIsDraft: true }, "pr"],
    ["Merge PR", {}, "merge-pr"],
  ] as const)(
    "lists %s from the main button in its group on the base branch, with the method row",
    (_case, overrides, id) => {
      const control = buildGitControl(openPullRequest({ isOnBaseBranch: true, ...overrides }));
      expect(control.primary).toMatchObject({ kind: "action", action: { id } });
      const pullRequest = control.sections.find((section) => section.id === "pull-request");
      expect(pullRequest?.actions.map((action) => action.id)).toContain(id);
      expect(control.mergeMethod).not.toBeNull();
    },
  );

  it("returns nothing outside a Git checkout", () => {
    expect(buildGitControl(createInput({ isGit: false }))).toEqual({
      primary: null,
      sections: [],
      mergeMethod: null,
      summary: null,
    });
  });
});

describe("buildGitControl summary", () => {
  it("states the branch, its base and the checkout facts", () => {
    const control = buildGitControl(
      openPullRequest({
        remoteAheadCount: 2,
        remoteBehindCount: 1,
        aheadOfOrigin: 2,
        behindOfOrigin: 1,
      }),
    );
    expect(control.summary).toEqual({
      branch: "snobbish-lizard",
      base: "main",
      facts: ["2 ahead", "1 behind", "Clean", "PR #812"],
    });
  });

  it("has no base on the base branch and notes an unpushed branch", () => {
    const control = buildGitControl(
      createInput({
        hasUncommittedChanges: true,
        remoteRef: null,
        remoteAheadCount: null,
        remoteBehindCount: null,
        forgeChangeRequestNoun: "MR",
        pullRequestNumberPrefix: "!",
      }),
    );
    expect(control.summary).toEqual({
      branch: "main",
      base: null,
      facts: ["Not pushed yet", "Uncommitted changes"],
    });
  });

  it("uses the forge's number prefix for the change request", () => {
    const control = buildGitControl(
      openPullRequest({ forgeChangeRequestNoun: "MR", pullRequestNumberPrefix: "!" }),
    );
    expect(control.summary?.facts).toContain("MR !812");
  });
});

import { describe, expect, it } from "vitest";
import { CheckoutPrStatusSchema } from "@ait/protocol/messages";
import { TURN_LIVENESS_IDLE } from "@/timeline/turn-liveness";
import type { CheckoutStatusPayload } from "./checkout-status-cache";
import { deriveWorkspaceActionState, hasActiveWorkspaceAgents } from "./workspace-action-state";

const HEAD = "a".repeat(40);
const NEW_HEAD = "b".repeat(40);

function gitStatus(overrides: Partial<CheckoutStatusPayload> = {}): CheckoutStatusPayload {
  return {
    cwd: "/repo",
    error: null,
    requestId: "status",
    isGit: true,
    isPaseoOwnedWorktree: true,
    repoRoot: "/repo",
    mainRepoRoot: "/main",
    currentBranch: "feature",
    isDirty: false,
    baseRef: "main",
    aheadBehind: { ahead: 2, behind: 5 },
    aheadOfOrigin: 0,
    behindOfOrigin: 0,
    hasRemote: true,
    remoteUrl: "git@github.com:example/repo.git",
    branchStatus: {
      headSha: HEAD,
      hasConflicts: false,
      remoteRef: "refs/remotes/origin/feature",
      aheadBehind: { ahead: 0, behind: 0 },
    },
    ...overrides,
  } as CheckoutStatusPayload;
}

function prStatus(overrides = {}) {
  return CheckoutPrStatusSchema.parse({
    url: "https://github.com/example/repo/pull/1",
    title: "Change",
    state: "merged",
    isMerged: true,
    baseRefName: "main",
    headRefName: "feature",
    headSha: HEAD,
    ...overrides,
  });
}

function derive(overrides: Partial<Parameters<typeof deriveWorkspaceActionState>[0]> = {}) {
  return deriveWorkspaceActionState({
    gitStatus: gitStatus(),
    prStatus: prStatus(),
    prStatusKnown: true,
    hasActiveAgents: false,
    ...overrides,
  });
}

describe("workspace action state", () => {
  it("offers archive for the merged source commit even when main has advanced or history differs", () => {
    expect(derive()).toMatchObject({
      shouldPromoteArchive: true,
      hasNewWork: false,
      hasOpenPullRequest: false,
    });
  });

  it.each(["closed", "merged"])(
    "restarts delivery after a %s PR, including commits already pushed",
    (state) => {
      const git = gitStatus();
      if (!git.branchStatus) throw new Error("missing fixture branch status");
      git.branchStatus.headSha = NEW_HEAD;
      expect(
        derive({ gitStatus: git, prStatus: prStatus({ state, isMerged: state === "merged" }) }),
      ).toMatchObject({ hasNewWork: true, hasOpenPullRequest: false, shouldPromoteArchive: false });
    },
  );

  it("does not treat a closed, unchanged PR as merged or new work", () => {
    expect(derive({ prStatus: prStatus({ state: "closed", isMerged: false }) })).toMatchObject({
      hasNewWork: false,
      hasOpenPullRequest: false,
      shouldPromoteArchive: false,
    });
  });

  it("keeps an open PR usable when local commits are newer than the PR head", () => {
    expect(
      derive({ prStatus: prStatus({ state: "open", isMerged: false, headSha: NEW_HEAD }) }),
    ).toMatchObject({ hasOpenPullRequest: true, hasNewWork: false, shouldPromoteArchive: false });
  });

  it("allows a deleted remote branch only with matching merged source evidence", () => {
    const git = gitStatus({
      branchStatus: { headSha: HEAD, hasConflicts: false, remoteRef: null, aheadBehind: null },
    });
    expect(derive({ gitStatus: git }).shouldPromoteArchive).toBe(true);
    expect(
      derive({ gitStatus: git, prStatus: prStatus({ headSha: NEW_HEAD }) }).shouldPromoteArchive,
    ).toBe(false);
  });

  it.each([
    { gitStatus: gitStatus({ isDirty: true }) },
    { hasActiveAgents: true },
    { prStatusKnown: false },
    { prStatus: null },
    { gitStatus: gitStatus({ branchStatus: undefined }) },
    { prStatus: prStatus({ headSha: undefined }) },
    { gitStatus: gitStatus({ error: { code: "UNKNOWN", message: "read failed" } }) },
  ])("does not recommend archive without complete, idle, clean evidence: %j", (input) => {
    expect(derive(input).shouldPromoteArchive).toBe(false);
  });

  it.each([
    { ahead: 1, behind: 0 },
    { ahead: 0, behind: 1 },
    { ahead: 1, behind: 1 },
  ])("does not recommend archive with remote differences: %j", (aheadBehind) => {
    expect(
      derive({
        gitStatus: gitStatus({
          branchStatus: {
            headSha: HEAD,
            hasConflicts: false,
            remoteRef: "refs/remotes/origin/feature",
            aheadBehind,
          },
        }),
      }).shouldPromoteArchive,
    ).toBe(false);
  });

  it("uses same-name counts even if the configured upstream is a different branch", () => {
    expect(
      derive({ gitStatus: gitStatus({ aheadOfOrigin: 5, behindOfOrigin: 10 }) }),
    ).toMatchObject({ remoteAheadCount: 0, remoteBehindCount: 0, shouldPromoteArchive: true });
  });

  it("identifies first-PR work and leaves empty experiments for manual archive", () => {
    expect(derive({ prStatus: null }).hasNewWork).toBe(true);
    expect(
      derive({ prStatus: null, gitStatus: gitStatus({ aheadBehind: { ahead: 0, behind: 0 } }) })
        .hasNewWork,
    ).toBe(false);
  });
});

describe("active workspace agents", () => {
  const idle = {
    workspaceId: "workspace",
    cwd: "/repo",
    archivedAt: null,
    status: "idle" as const,
    turn: TURN_LIVENESS_IDLE,
    pendingPermissions: [],
  };
  const active = {
    ...idle,
    turn: { phase: "open" as const, turnId: "turn", startedAt: null, cancellationRequestId: null },
  };
  const hasActive = (agents: Parameters<typeof hasActiveWorkspaceAgents>[0]["agents"]) =>
    hasActiveWorkspaceAgents({ agents, workspaceId: "workspace", cwd: "/repo" });

  it("checks every agent, including children, even when a later agent is idle", () => {
    expect(hasActive([active, idle])).toBe(true);
    expect(hasActive([idle])).toBe(false);
  });

  it("includes initialization and excludes archived or other-workspace agents", () => {
    expect(hasActive([{ ...idle, status: "initializing" }])).toBe(true);
    expect(hasActive([{ ...idle, status: "running" }])).toBe(true);
    expect(hasActive([{ ...idle, pendingPermissions: [{}] }])).toBe(true);
    expect(hasActive([{ ...active, archivedAt: new Date() }])).toBe(false);
    expect(hasActive([{ ...active, workspaceId: "another" }])).toBe(false);
    expect(hasActive([{ ...active, workspaceId: undefined }])).toBe(true);
  });
});

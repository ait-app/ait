// @vitest-environment jsdom
import React, { type ReactNode } from "react";
import { cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n/i18next";
import type { ToastApi } from "@/components/toast-host";
import { ToastApiProvider } from "@/contexts/toast-context";
import { useSessionStore } from "@/stores/session-store";
import { listGitControlActions, type GitControlAction } from "./control";
import { useGitActionRunner, useGitActions } from "./use-actions";

const queries = vi.hoisted(() => ({
  checkoutStatus: null as unknown,
  prStatus: null as unknown,
}));
const confirmDialog = vi.hoisted(() => vi.fn(async () => false));

vi.mock("@react-native-async-storage/async-storage", () => ({
  default: {
    getItem: vi.fn(async () => null),
    setItem: vi.fn(async () => undefined),
    removeItem: vi.fn(async () => undefined),
  },
}));
vi.mock("@/git/use-status-query", () => ({
  useCheckoutStatusQuery: () => ({
    status: queries.checkoutStatus,
    isLoading: false,
    isFetching: false,
    isError: false,
    error: null,
  }),
}));
vi.mock("@/git/use-pr-status-query", () => ({
  useCheckoutPrStatusQuery: () => ({
    status: queries.prStatus,
    githubFeaturesEnabled: true,
    forge: "github",
    hasData: true,
    isLoading: false,
    isFetching: false,
    isError: false,
    payloadError: null,
  }),
}));
vi.mock("@/utils/confirm-dialog", () => ({ confirmDialog }));
vi.mock("@/stores/navigation-active-workspace-store", () => ({
  useActiveWorkspaceSelection: () => null,
}));

const serverId = "server-1";
const cwd = "/repo/worktrees/feature";

function toastApi(): ToastApi {
  return { show: vi.fn(), copied: vi.fn(), error: vi.fn() };
}

function withToasts(api: ToastApi) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return <ToastApiProvider api={api}>{children}</ToastApiProvider>;
  };
}

function action(overrides: Partial<GitControlAction> = {}): GitControlAction {
  return {
    id: "pull",
    label: "Pull",
    pendingLabel: "Pulling...",
    successLabel: "Pulled",
    status: "idle",
    disabled: false,
    handler: vi.fn(),
    ...overrides,
  };
}

/** A managed worktree whose PR is merged and which is clean and in sync with its remote copy. */
function seedDeliveredWorkspace() {
  queries.checkoutStatus = {
    cwd,
    requestId: "status",
    isGit: true,
    error: null,
    isDirty: false,
    baseRef: "main",
    currentBranch: "feature",
    aheadBehind: { ahead: 0, behind: 0 },
    aheadOfOrigin: 0,
    behindOfOrigin: 0,
    hasRemote: true,
    isPaseoOwnedWorktree: true,
    upstreamRef: "refs/remotes/origin/feature",
    branchStatus: {
      headSha: "abc123",
      hasConflicts: false,
      remoteRef: "refs/remotes/origin/feature",
      aheadBehind: { ahead: 0, behind: 0 },
    },
  };
  queries.prStatus = {
    url: "https://github.com/acme/repo/pull/7",
    number: 7,
    state: "merged",
    isMerged: true,
    isDraft: false,
    mergeable: "UNKNOWN",
    headSha: "abc123",
    headRefName: "feature",
  };
  const workspace = {
    id: "workspace-1",
    projectId: "project-1",
    workspaceDirectory: cwd,
    initialBranch: "feature",
  };
  useSessionStore.setState((state) => ({
    ...state,
    sessions: {
      [serverId]: {
        workspaces: new Map([[workspace.id, workspace]]),
        agents: new Map(),
        hasHydratedAgents: true,
        serverInfo: { features: {} },
      } as unknown as (typeof state.sessions)[string],
    },
  }));
}

function resetAction(api: ToastApi): GitControlAction {
  const { result } = renderHook(() => useGitActions({ serverId, cwd }), {
    wrapper: withToasts(api),
  });
  const reset = listGitControlActions(result.current.gitControl).find(
    (candidate) => candidate.id === "reset-workspace",
  );
  if (!reset) throw new Error("reset-workspace is not in the panel");
  return reset;
}

beforeEach(() => {
  vi.stubGlobal("React", React);
  confirmDialog.mockClear();
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  useSessionStore.setState((state) => ({ ...state, sessions: {} }));
  queries.checkoutStatus = null;
  queries.prStatus = null;
});

describe("useGitActionRunner", () => {
  it("does nothing for a disabled action", () => {
    const api = toastApi();
    const disabled = action({ disabled: true, unavailableReason: "Already up to date" });
    const { result } = renderHook(() => useGitActionRunner(), { wrapper: withToasts(api) });

    result.current(disabled);

    expect(disabled.handler).not.toHaveBeenCalled();
    expect(api.show).not.toHaveBeenCalled();
  });

  it("shows the reason of an unavailable action instead of running it", () => {
    const api = toastApi();
    const unavailable = action({ unavailableReason: "Already up to date" });
    const { result } = renderHook(() => useGitActionRunner(), { wrapper: withToasts(api) });

    result.current(unavailable);

    expect(unavailable.handler).not.toHaveBeenCalled();
    expect(api.show).toHaveBeenCalledWith("Already up to date", expect.any(Object));
  });

  it("runs an available action", () => {
    const api = toastApi();
    const available = action();
    const { result } = renderHook(() => useGitActionRunner(), { wrapper: withToasts(api) });

    result.current(available);

    expect(available.handler).toHaveBeenCalledOnce();
    expect(api.show).not.toHaveBeenCalled();
  });
});

describe("useGitActions reset confirmation", () => {
  it("asks the short question for a delivered workspace", () => {
    seedDeliveredWorkspace();

    resetAction(toastApi()).handler();

    expect(confirmDialog).toHaveBeenCalledOnce();
    expect(confirmDialog).toHaveBeenCalledWith(
      expect.objectContaining({
        message: "Your work is merged. Reset this workspace to the latest main?",
        destructive: true,
      }),
    );
  });

  it("keeps the full warning when the workspace is not delivered", () => {
    seedDeliveredWorkspace();
    queries.checkoutStatus = { ...(queries.checkoutStatus as object), isDirty: true };

    resetAction(toastApi()).handler();

    expect(confirmDialog).toHaveBeenCalledWith(
      expect.objectContaining({
        message: expect.stringContaining("restore branch feature"),
        destructive: true,
      }),
    );
  });
});

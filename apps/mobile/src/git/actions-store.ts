import type { CheckoutPrMergeMethod } from "@ait/protocol/messages";
import { create } from "zustand";
import { queryClient as appQueryClient } from "@/data/query-client";
import { useSessionStore } from "@/stores/session-store";
import { invalidateCheckoutGitQueriesForClient } from "@/git/query-keys";
import { i18n } from "@/i18n/i18next";

const SUCCESS_DISPLAY_MS = 1000;

export type CheckoutGitActionStatus = "idle" | "pending" | "success";

export type CheckoutGitAsyncActionId =
  | "commit"
  | "pull"
  | "push"
  | "pull-and-push"
  | "refresh"
  | "create-pr"
  | "merge-pr-squash"
  | "merge-pr-merge"
  | "merge-pr-rebase"
  | "enable-pr-auto-merge-squash"
  | "enable-pr-auto-merge-merge"
  | "enable-pr-auto-merge-rebase"
  | "disable-pr-auto-merge"
  | "merge-branch"
  | "reset-workspace"
  | "discard-changes";

type CheckoutKey = string;
type StatusMap = Partial<Record<CheckoutGitAsyncActionId, CheckoutGitActionStatus>>;

function checkoutKey(serverId: string, cwd: string): CheckoutKey {
  return `${serverId}::${cwd}`;
}

function resolveClient(serverId: string) {
  const session = useSessionStore.getState().sessions[serverId];
  const client = session?.client ?? null;
  if (!client) {
    throw new Error(i18n.t("common.errors.daemonClientUnavailable"));
  }
  return client;
}

type AutoMergeActionsRpc = "forge" | "github";

function resolveAutoMergeActionsRpc(serverId: string): AutoMergeActionsRpc {
  const session = useSessionStore.getState().sessions[serverId];
  if (session?.serverInfo?.features?.checkoutForgeSetAutoMerge === true) {
    return "forge";
  }
  // COMPAT(githubAutoMergeRpc): use the legacy GitHub RPC with daemons that
  // predate checkout.forge.set_auto_merge.*. Remove after 2027-01-17 once the
  // supported daemon floor is >= v0.2.0.
  if (session?.serverInfo?.features?.checkoutGithubSetAutoMerge === true) {
    return "github";
  }
  throw new Error("Update the host to use auto-merge actions.");
}

function setStatus(
  key: CheckoutKey,
  actionId: CheckoutGitAsyncActionId,
  status: CheckoutGitActionStatus,
) {
  useCheckoutGitActionsStore.setState((state) => {
    const current = state.statusByCheckout[key]?.[actionId] ?? "idle";
    if (current === status) {
      return state;
    }
    return {
      ...state,
      statusByCheckout: {
        ...state.statusByCheckout,
        [key]: {
          ...state.statusByCheckout[key],
          [actionId]: status,
        },
      },
    };
  });
}

function invalidateCheckoutGitQueries(serverId: string, cwd: string) {
  return invalidateCheckoutGitQueriesForClient(appQueryClient, { serverId, cwd });
}

const successTimers = new Map<string, ReturnType<typeof setTimeout>>();
const inFlight = new Map<string, Promise<unknown>>();
/**
 * Status of each request, grouped by checkout action. A parameterized action such as
 * `discard-changes` can run several requests at once; the action stays pending until all of them
 * settle, so the first to finish cannot mark the others successful.
 */
const requestStatusesByAction = new Map<string, Map<string, CheckoutGitActionStatus>>();

function aggregateActionStatus(
  requestStatuses: ReadonlyMap<string, CheckoutGitActionStatus>,
): CheckoutGitActionStatus {
  let hasSuccess = false;
  for (const status of requestStatuses.values()) {
    if (status === "pending") return "pending";
    if (status === "success") hasSuccess = true;
  }
  return hasSuccess ? "success" : "idle";
}

function setRequestStatus(
  key: CheckoutKey,
  actionId: CheckoutGitAsyncActionId,
  requestId: string,
  status: CheckoutGitActionStatus,
) {
  const actionKey = inFlightKey(key, actionId, undefined);
  const requestStatuses = requestStatusesByAction.get(actionKey) ?? new Map();
  if (status === "idle") {
    requestStatuses.delete(requestId);
  } else {
    requestStatuses.set(requestId, status);
  }
  if (requestStatuses.size === 0) {
    requestStatusesByAction.delete(actionKey);
  } else {
    requestStatusesByAction.set(actionKey, requestStatuses);
  }
  setStatus(key, actionId, aggregateActionStatus(requestStatuses));
}

function inFlightKey(
  key: CheckoutKey,
  actionId: CheckoutGitAsyncActionId,
  variant: string | undefined,
): string {
  return variant === undefined ? `${key}::${actionId}` : `${key}::${actionId}::${variant}`;
}

interface CheckoutGitActionsStoreState {
  statusByCheckout: Record<CheckoutKey, StatusMap>;

  getStatus: (params: {
    serverId: string;
    cwd: string;
    actionId: CheckoutGitAsyncActionId;
  }) => CheckoutGitActionStatus;

  commit: (params: { serverId: string; cwd: string }) => Promise<void>;
  pull: (params: { serverId: string; cwd: string }) => Promise<void>;
  push: (params: { serverId: string; cwd: string }) => Promise<void>;
  pullAndPush: (params: { serverId: string; cwd: string }) => Promise<void>;
  refresh: (params: { serverId: string; cwd: string }) => Promise<void>;
  createPr: (params: { serverId: string; cwd: string }) => Promise<void>;
  mergePr: (params: {
    serverId: string;
    cwd: string;
    method: CheckoutPrMergeMethod;
  }) => Promise<void>;
  enablePrAutoMerge: (params: {
    serverId: string;
    cwd: string;
    method: CheckoutPrMergeMethod;
  }) => Promise<void>;
  disablePrAutoMerge: (params: { serverId: string; cwd: string }) => Promise<void>;
  mergeBranch: (params: { serverId: string; cwd: string; baseRef: string }) => Promise<void>;
  resetWorkspace: (params: {
    serverId: string;
    cwd: string;
    workspaceId: string;
    initialBranch: string;
  }) => Promise<void>;
  discardChanges: (params: { serverId: string; cwd: string; paths: string[] }) => Promise<void>;
}

async function runCheckoutAction({
  serverId,
  cwd,
  actionId,
  variant,
  run,
}: {
  serverId: string;
  cwd: string;
  actionId: CheckoutGitAsyncActionId;
  // Parameterized actions only share an in-flight request when their parameters match.
  variant?: string;
  run: () => Promise<void>;
}): Promise<void> {
  const key = checkoutKey(serverId, cwd);
  const inflightId = inFlightKey(key, actionId, variant);

  const existing = inFlight.get(inflightId);
  if (existing) {
    await existing;
    return;
  }

  const prevTimer = successTimers.get(inflightId);
  if (prevTimer) {
    clearTimeout(prevTimer);
    successTimers.delete(inflightId);
  }

  setRequestStatus(key, actionId, inflightId, "pending");

  const promise = (async () => {
    try {
      await run();
      await invalidateCheckoutGitQueries(serverId, cwd);
      setRequestStatus(key, actionId, inflightId, "success");
      const timer = setTimeout(() => {
        setRequestStatus(key, actionId, inflightId, "idle");
        successTimers.delete(inflightId);
      }, SUCCESS_DISPLAY_MS);
      successTimers.set(inflightId, timer);
    } catch (error) {
      setRequestStatus(key, actionId, inflightId, "idle");
      throw error;
    } finally {
      inFlight.delete(inflightId);
    }
  })();

  inFlight.set(inflightId, promise);
  await promise;
}

export const useCheckoutGitActionsStore = create<CheckoutGitActionsStoreState>()((set, get) => ({
  statusByCheckout: {},

  getStatus: ({ serverId, cwd, actionId }) => {
    const key = checkoutKey(serverId, cwd);
    return get().statusByCheckout[key]?.[actionId] ?? "idle";
  },

  commit: async ({ serverId, cwd }) => {
    await runCheckoutAction({
      serverId,
      cwd,
      actionId: "commit",
      run: async () => {
        const client = resolveClient(serverId);
        const payload = await client.checkoutCommit(cwd, { addAll: true });
        if (payload.error) {
          throw new Error(payload.error.message);
        }
      },
    });
  },

  pull: async ({ serverId, cwd }) => {
    await runCheckoutAction({
      serverId,
      cwd,
      actionId: "pull",
      run: async () => {
        const client = resolveClient(serverId);
        const payload = await client.checkoutPull(cwd);
        if (payload.error) {
          throw new Error(payload.error.message);
        }
      },
    });
  },

  push: async ({ serverId, cwd }) => {
    await runCheckoutAction({
      serverId,
      cwd,
      actionId: "push",
      run: async () => {
        const client = resolveClient(serverId);
        const payload = await client.checkoutPush(cwd);
        if (payload.error) {
          throw new Error(payload.error.message);
        }
      },
    });
  },

  refresh: async ({ serverId, cwd }) => {
    await runCheckoutAction({
      serverId,
      cwd,
      actionId: "refresh",
      run: async () => {
        const client = resolveClient(serverId);
        const payload = await client.checkoutRefresh(cwd);
        if (payload.error) {
          throw new Error(payload.error.message);
        }
      },
    });
  },

  pullAndPush: async ({ serverId, cwd }) => {
    await runCheckoutAction({
      serverId,
      cwd,
      actionId: "pull-and-push",
      run: async () => {
        const client = resolveClient(serverId);
        const pullPayload = await client.checkoutPull(cwd);
        if (pullPayload.error) {
          throw new Error(pullPayload.error.message);
        }
        const pushPayload = await client.checkoutPush(cwd);
        if (pushPayload.error) {
          throw new Error(pushPayload.error.message);
        }
      },
    });
  },

  createPr: async ({ serverId, cwd }) => {
    await runCheckoutAction({
      serverId,
      cwd,
      actionId: "create-pr",
      run: async () => {
        const client = resolveClient(serverId);
        const payload = await client.checkoutPrCreate(cwd, {});
        if (payload.error) {
          throw new Error(payload.error.message);
        }
      },
    });
  },

  mergePr: async ({ serverId, cwd, method }) => {
    await runCheckoutAction({
      serverId,
      cwd,
      actionId: `merge-pr-${method}`,
      run: async () => {
        const client = resolveClient(serverId);
        const payload = await client.checkoutPrMerge(cwd, { method });
        if (payload.error) {
          throw new Error(payload.error.message);
        }
      },
    });
  },

  enablePrAutoMerge: async ({ serverId, cwd, method }) => {
    const rpc = resolveAutoMergeActionsRpc(serverId);
    await runCheckoutAction({
      serverId,
      cwd,
      actionId: `enable-pr-auto-merge-${method}`,
      run: async () => {
        const client = resolveClient(serverId);
        // COMPAT(githubAutoMergeRpc): use the legacy GitHub RPC with daemons
        // that predate checkout.forge.set_auto_merge.*. Remove after 2027-01-17
        // once the supported daemon floor is >= v0.2.0.
        const payload =
          rpc === "forge"
            ? await client.checkoutForgeSetAutoMerge(cwd, { enabled: true, method })
            : await client.checkoutGithubSetAutoMerge(cwd, { enabled: true, method });
        if (payload.error) {
          throw new Error(payload.error.message);
        }
      },
    });
  },

  disablePrAutoMerge: async ({ serverId, cwd }) => {
    const rpc = resolveAutoMergeActionsRpc(serverId);
    await runCheckoutAction({
      serverId,
      cwd,
      actionId: "disable-pr-auto-merge",
      run: async () => {
        const client = resolveClient(serverId);
        // COMPAT(githubAutoMergeRpc): use the legacy GitHub RPC with daemons
        // that predate checkout.forge.set_auto_merge.*. Remove after 2027-01-17
        // once the supported daemon floor is >= v0.2.0.
        const payload =
          rpc === "forge"
            ? await client.checkoutForgeSetAutoMerge(cwd, { enabled: false })
            : await client.checkoutGithubSetAutoMerge(cwd, { enabled: false });
        if (payload.error) {
          throw new Error(payload.error.message);
        }
      },
    });
  },

  mergeBranch: async ({ serverId, cwd, baseRef }) => {
    await runCheckoutAction({
      serverId,
      cwd,
      actionId: "merge-branch",
      variant: baseRef,
      run: async () => {
        const client = resolveClient(serverId);
        const payload = await client.checkoutMerge(cwd, {
          baseRef,
          strategy: "merge",
          requireCleanTarget: true,
        });
        if (payload.error) {
          throw new Error(payload.error.message);
        }
      },
    });
  },

  resetWorkspace: async ({ serverId, cwd, workspaceId, initialBranch }) => {
    await runCheckoutAction({
      serverId,
      cwd,
      actionId: "reset-workspace",
      run: async () => {
        const client = resolveClient(serverId);
        const payload = await client.checkoutResetWorkspace(cwd, workspaceId, initialBranch);
        if (payload.error) {
          throw new Error(payload.error.message);
        }
      },
    });
  },

  discardChanges: async ({ serverId, cwd, paths }) => {
    await runCheckoutAction({
      serverId,
      cwd,
      actionId: "discard-changes",
      variant: JSON.stringify(paths),
      run: async () => {
        const client = resolveClient(serverId);
        const payload = await client.checkoutDiscardChanges(cwd, { paths });
        if (!payload.success) {
          throw new Error(
            payload.error?.message ?? i18n.t("workspace.fileActions.confirmRevert.failed"),
          );
        }
      },
    });
  },
}));

export function __resetCheckoutGitActionsStoreForTests() {
  for (const timer of successTimers.values()) {
    clearTimeout(timer);
  }
  successTimers.clear();
  inFlight.clear();
  requestStatusesByAction.clear();
  useCheckoutGitActionsStore.setState({ statusByCheckout: {} });
}

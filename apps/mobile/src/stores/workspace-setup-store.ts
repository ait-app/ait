import { createNameId } from "mnemonic-id";
import { generateDraftId } from "@/stores/draft-keys";
import type { SessionOutboundMessage } from "@ait/protocol/messages";
import { create } from "zustand";
import { buildWorkspaceTabPersistenceKey } from "@/workspace-tabs/model";

export type WorkspaceCreationMethod = "open_project" | "create_worktree";

export interface PendingWorkspaceSetup {
  serverId: string;
  sourceDirectory: string;
  sourceWorkspaceId?: string;
  displayName?: string;
  creationMethod: WorkspaceCreationMethod;
}

export type WorkspaceSetupProgressPayload = Extract<
  SessionOutboundMessage,
  { type: "workspace_setup_progress" }
>["payload"];

export type WorkspaceSetupStatusResult = Extract<
  SessionOutboundMessage,
  { type: "workspace.setup.status.response" }
>["payload"];

export type WorkspaceSetupSummary = Extract<
  SessionOutboundMessage,
  { type: "workspace_setup_status" }
>["payload"];

export interface WorkspaceSetupStatusClient {
  fetchWorkspaceSetupStatus: (workspaceId: string) => Promise<WorkspaceSetupStatusResult>;
  fetchWorkspaceSetupSummary?: (
    workspaceId: string,
  ) => Promise<
    Extract<SessionOutboundMessage, { type: "workspace.setup.summary.response" }>["payload"]
  >;
}

export interface WorkspaceSetupSnapshot extends WorkspaceSetupProgressPayload {
  updatedAt: number;
}

export function shouldShowWorkspaceSetup(
  snapshot: WorkspaceSetupSnapshot | WorkspaceSetupSummary | null,
): boolean {
  if (!snapshot) {
    return false;
  }
  return "hasCommands" in snapshot
    ? snapshot.status !== "completed" || snapshot.hasCommands
    : snapshot.status === "blocked" ||
        snapshot.error !== null ||
        snapshot.detail.commands.length > 0;
}

export function shouldSeedWorkspaceSetupTab(
  snapshot: WorkspaceSetupSnapshot | WorkspaceSetupSummary | null,
): boolean {
  return snapshot?.status === "failed" || snapshot?.status === "blocked";
}

interface WorkspaceSetupStoreState {
  pendingWorkspaceSetup:
    | (PendingWorkspaceSetup & { creationId: string; worktreeSlug: string })
    | null;
  snapshots: Record<string, WorkspaceSetupSnapshot>;
  summaries: Record<string, WorkspaceSetupSummary>;
  serverGenerations: Record<string, number>;
  upsertSummary: (input: { serverId: string; payload: WorkspaceSetupSummary }) => void;
  applySnapshot: (input: {
    serverId: string;
    workspaceId: string;
    snapshot: WorkspaceSetupStatusResult["snapshot"];
  }) => void;

  requestedKeys: Set<string>;
  surfacedFailedSetupKeys: Set<string>;
  beginWorkspaceSetup: (value: PendingWorkspaceSetup) => void;
  clearWorkspaceSetup: () => void;
  upsertProgress: (input: { serverId: string; payload: WorkspaceSetupProgressPayload }) => void;
  claimFailedSetupSurface: (input: {
    serverId: string;
    workspaceId: string;
    status?: WorkspaceSetupSummary | WorkspaceSetupSnapshot | null;
  }) => boolean;
  ensureSetupStatus: (input: {
    serverId: string;
    workspaceId: string;
    client: WorkspaceSetupStatusClient;
    summaryOnly?: boolean;
  }) => void;
  removeWorkspace: (input: { serverId: string; workspaceId: string }) => void;
  clearServer: (serverId: string) => void;
}

function buildWorkspaceSetupKey(input: { serverId: string; workspaceId: string }): string | null {
  return buildWorkspaceTabPersistenceKey(input);
}

export const useWorkspaceSetupStore = create<WorkspaceSetupStoreState>()((set, get) => ({
  pendingWorkspaceSetup: null,
  snapshots: {},
  summaries: {},
  serverGenerations: {},
  requestedKeys: new Set(),
  surfacedFailedSetupKeys: new Set(),
  beginWorkspaceSetup: (value) => {
    set({
      pendingWorkspaceSetup: {
        ...value,
        creationId: generateDraftId(),
        worktreeSlug: createNameId(),
      },
    });
  },
  clearWorkspaceSetup: () => {
    set({ pendingWorkspaceSetup: null });
  },
  upsertSummary: ({ serverId, payload }) => {
    const key = buildWorkspaceSetupKey({ serverId, workspaceId: payload.workspaceId });
    if (!key) return;
    set((state) => {
      const previous = state.summaries[key];
      if (
        previous?.revision !== undefined &&
        payload.revision !== undefined &&
        payload.revision < previous.revision
      )
        return state;
      const surfacedFailedSetupKeys = new Set(state.surfacedFailedSetupKeys);
      if (payload.status !== "failed" && payload.status !== "blocked")
        surfacedFailedSetupKeys.delete(key);
      return { summaries: { ...state.summaries, [key]: payload }, surfacedFailedSetupKeys };
    });
  },
  applySnapshot: ({ serverId, workspaceId, snapshot }) => {
    if (snapshot) {
      get().upsertProgress({ serverId, payload: { workspaceId, ...snapshot } });
      return;
    }
    get().removeWorkspace({ serverId, workspaceId });
  },
  upsertProgress: ({ serverId, payload }) => {
    const key = buildWorkspaceSetupKey({ serverId, workspaceId: payload.workspaceId });
    if (!key) {
      return;
    }

    const previous = get().snapshots[key];
    const summary = get().summaries[key];
    if (
      summary?.revision !== undefined &&
      payload.revision !== undefined &&
      payload.revision < summary.revision
    )
      return;
    if (
      previous?.revision !== undefined &&
      payload.revision !== undefined &&
      payload.revision < previous.revision
    )
      return;
    get().upsertSummary({
      serverId,
      payload: {
        workspaceId: payload.workspaceId,
        status: payload.status,
        hasCommands: payload.detail.commands.length > 0,
        revision: payload.revision,
      },
    });
    set((state) => {
      const surfacedFailedSetupKeys = new Set(state.surfacedFailedSetupKeys);
      if (payload.status !== "failed" && payload.status !== "blocked") {
        surfacedFailedSetupKeys.delete(key);
      }
      return {
        snapshots: {
          ...state.snapshots,
          [key]: {
            ...payload,
            updatedAt: Date.now(),
          },
        },
        surfacedFailedSetupKeys,
      };
    });
  },
  claimFailedSetupSurface: ({ serverId, workspaceId, status }) => {
    const key = buildWorkspaceSetupKey({ serverId, workspaceId });
    if (!key) {
      return false;
    }

    let claimed = false;
    set((state) => {
      if (
        !["failed", "blocked"].includes(
          (status ?? state.summaries[key] ?? state.snapshots[key])?.status ?? "",
        ) ||
        state.surfacedFailedSetupKeys.has(key)
      ) {
        return state;
      }
      claimed = true;
      return { surfacedFailedSetupKeys: new Set(state.surfacedFailedSetupKeys).add(key) };
    });
    return claimed;
  },
  ensureSetupStatus: async ({ serverId, workspaceId, client, summaryOnly = false }) => {
    const key = buildWorkspaceSetupKey({ serverId, workspaceId });
    if (!key) {
      return;
    }
    const state = get();
    if (
      (summaryOnly ? state.summaries[key] : state.snapshots[key]) ||
      state.requestedKeys.has(key)
    ) {
      return;
    }

    // requestedKeys is a pure in-flight marker: it dedupes concurrent fetches and is
    // released once the request settles. A settle that stored no snapshot (null snapshot,
    // mismatched workspace, or error) leaves no marker, so a later call can retry; once a
    // snapshot lands, the snapshots[key] guard above prevents redundant refetches.
    set((current) => ({ requestedKeys: new Set(current.requestedKeys).add(key) }));

    const generation = get().serverGenerations[serverId] ?? 0;
    try {
      if (summaryOnly && client.fetchWorkspaceSetupSummary) {
        const response = await client.fetchWorkspaceSetupSummary(workspaceId);
        if ((get().serverGenerations[serverId] ?? 0) !== generation) return;
        if (response.workspaceId === workspaceId && response.summary)
          get().upsertSummary({ serverId, payload: { workspaceId, ...response.summary } });
        return;
      }
      const response = await client.fetchWorkspaceSetupStatus(workspaceId);
      if ((get().serverGenerations[serverId] ?? 0) !== generation) return;
      if (response.workspaceId === workspaceId && response.snapshot) {
        get().upsertProgress({
          serverId,
          payload: { workspaceId: response.workspaceId, ...response.snapshot },
        });
      }
    } catch {
      // Swallowed: the finally clears the in-flight marker so a later call retries.
    } finally {
      set((current) => {
        if ((current.serverGenerations[serverId] ?? 0) !== generation) return current;
        const next = new Set(current.requestedKeys);
        next.delete(key);
        return { requestedKeys: next };
      });
    }
  },
  removeWorkspace: ({ serverId, workspaceId }) => {
    const key = buildWorkspaceSetupKey({ serverId, workspaceId });
    if (!key) {
      return;
    }

    set((state) => {
      if (
        !(key in state.snapshots) &&
        !(key in state.summaries) &&
        !state.surfacedFailedSetupKeys.has(key)
      ) {
        return state;
      }
      const summaries = { ...state.summaries };
      delete summaries[key];
      const next = { ...state.snapshots };
      delete next[key];
      const surfacedFailedSetupKeys = new Set(state.surfacedFailedSetupKeys);
      surfacedFailedSetupKeys.delete(key);
      return { snapshots: next, summaries, surfacedFailedSetupKeys };
    });
  },
  clearServer: (serverId) => {
    set((state) => {
      const keep = (key: string) => !key.startsWith(`${serverId}:`);
      return {
        snapshots: Object.fromEntries(Object.entries(state.snapshots).filter(([key]) => keep(key))),
        summaries: Object.fromEntries(Object.entries(state.summaries).filter(([key]) => keep(key))),
        requestedKeys: new Set([...state.requestedKeys].filter(keep)),
        surfacedFailedSetupKeys: new Set([...state.surfacedFailedSetupKeys].filter(keep)),
        serverGenerations: {
          ...state.serverGenerations,
          [serverId]: (state.serverGenerations[serverId] ?? 0) + 1,
        },
      };
    });
  },
}));

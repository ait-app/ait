import type { AgentSnapshotPayload } from "@ait/protocol/messages";
import type { CreateWorkspaceRequestOptions } from "@ait/client/internal/daemon-client";
import { createNameId } from "mnemonic-id";
import { create } from "zustand";
import { generateDraftId } from "@/stores/draft-keys";
import type { ComposerAttachment } from "@/attachments/types";
import type { AgentProvider } from "@ait/protocol/agent-types";
import type { WorkspaceDraftTabSetup } from "@/workspace-tabs/model";

export interface PendingWorkspaceDraftSubmission {
  /** Already-running creation. Mounting the draft only observes its result. */
  agentCreation?: {
    clientMessageId: string;
    result: Promise<AgentSnapshotPayload>;
  };
  serverId: string;
  workspaceId: string;
  draftId: string;
  text: string;
  attachments: ComposerAttachment[];
  cwd: string;
  provider: AgentProvider;
  clientMessageId: string;
  timestamp: number;
  modeId?: string;
  model?: string;
  thinkingOptionId?: string;
  featureValues?: Record<string, unknown>;
}

export interface PendingWorkspaceDraftSetup {
  setup: WorkspaceDraftTabSetup;
  sourceDirectory?: string | null;
}

interface WorkspaceCreationAttempt {
  serverId: string;
  intent: string;
  idempotencyKey: string;
  worktreeSlug: string;
}

interface WorkspaceDraftSubmissionState {
  pendingByDraftId: Record<string, PendingWorkspaceDraftSubmission>;
  setupByDraftId: Record<string, PendingWorkspaceDraftSetup>;
  creationByDraftId: Record<string, NonNullable<PendingWorkspaceDraftSubmission["agentCreation"]>>;
  workspaceCreationByDraftId: Record<string, WorkspaceCreationAttempt>;
  prepareWorkspaceCreation: (input: {
    draftId: string;
    serverId: string;
    intent: Omit<CreateWorkspaceRequestOptions, "idempotencyKey" | "requestId" | "onEvent">;
  }) => WorkspaceCreationAttempt;
  setPending: (submission: PendingWorkspaceDraftSubmission) => void;
  setDraftSetup: (input: {
    draftId: string;
    setup: WorkspaceDraftTabSetup;
    sourceDirectory?: string | null;
  }) => void;
  clearDraftSetup: (input: { draftId: string }) => void;
  consumePending: (input: {
    serverId: string;
    workspaceId: string;
    draftId: string;
  }) => PendingWorkspaceDraftSubmission | null;
}

function matchesPendingSubmission(
  pending: PendingWorkspaceDraftSubmission | null | undefined,
  input: { serverId: string; workspaceId: string; draftId: string },
): pending is PendingWorkspaceDraftSubmission {
  return (
    pending?.serverId === input.serverId &&
    pending.workspaceId === input.workspaceId &&
    pending.draftId === input.draftId
  );
}

function normalizeDraftId(draftId: string): string {
  return draftId.trim();
}

export const useWorkspaceDraftSubmissionStore = create<WorkspaceDraftSubmissionState>(
  (set, get) => ({
    pendingByDraftId: {},
    setupByDraftId: {},
    creationByDraftId: {},
    workspaceCreationByDraftId: {},
    prepareWorkspaceCreation: ({ draftId, serverId, intent }) => {
      const normalizedDraftId = normalizeDraftId(draftId);
      // Compare the complete request before adding generated IDs. Object key order and
      // undefined fields do not change the intent transmitted as JSON.
      const serializedIntent = JSON.stringify(intent, (_key, value: unknown) =>
        value && typeof value === "object" && !Array.isArray(value)
          ? Object.fromEntries(
              Object.entries(value).sort(([left], [right]) => left.localeCompare(right)),
            )
          : value,
      );
      const previous = get().workspaceCreationByDraftId[normalizedDraftId];
      if (previous?.serverId === serverId && previous.intent === serializedIntent) return previous;
      const attempt = {
        serverId,
        intent: serializedIntent,
        idempotencyKey: generateDraftId(),
        worktreeSlug: createNameId(),
      };
      set((state) => ({
        workspaceCreationByDraftId: {
          ...state.workspaceCreationByDraftId,
          [normalizedDraftId]: attempt,
        },
      }));
      return attempt;
    },
    setPending: (submission) =>
      set((state) => ({
        creationByDraftId: submission.agentCreation
          ? { ...state.creationByDraftId, [submission.draftId]: submission.agentCreation }
          : state.creationByDraftId,
        pendingByDraftId: {
          ...state.pendingByDraftId,
          [submission.draftId]: submission,
        },
      })),
    setDraftSetup: ({ draftId, setup, sourceDirectory }) => {
      const normalizedDraftId = normalizeDraftId(draftId);
      if (!normalizedDraftId) return;
      set((state) => ({
        setupByDraftId: {
          ...state.setupByDraftId,
          [normalizedDraftId]: { setup, sourceDirectory: sourceDirectory ?? null },
        },
      }));
    },
    clearDraftSetup: ({ draftId }) => {
      const normalizedDraftId = normalizeDraftId(draftId);
      if (!normalizedDraftId) return;
      set((state) => {
        const creationByDraftId = { ...state.creationByDraftId };
        delete creationByDraftId[normalizedDraftId];
        const workspaceCreationByDraftId = { ...state.workspaceCreationByDraftId };
        delete workspaceCreationByDraftId[normalizedDraftId];
        const { [normalizedDraftId]: _removed, ...setupByDraftId } = state.setupByDraftId;
        return { setupByDraftId, creationByDraftId, workspaceCreationByDraftId };
      });
    },
    consumePending: (input) => {
      const pending = get().pendingByDraftId[input.draftId];
      if (!matchesPendingSubmission(pending, input)) {
        return null;
      }
      set((state) => {
        if (!matchesPendingSubmission(state.pendingByDraftId[input.draftId], input)) {
          return state;
        }
        // Expo miscompiles computed member-key rest omissions; test the emitted code.
        const pendingByDraftId = { ...state.pendingByDraftId };
        delete pendingByDraftId[input.draftId];
        return { pendingByDraftId };
      });
      return pending;
    },
  }),
);

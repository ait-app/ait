import { create, type StateCreator } from "zustand";
import { persist } from "zustand/middleware";
import AsyncStorage from "@react-native-async-storage/async-storage";
import type { AttachmentMetadata, WorkspaceFileComposerAttachment } from "@/attachments/types";
import { appendWorkspaceFileAttachment } from "@/attachments/workspace-file";
import {
  garbageCollectAttachments,
  persistAttachmentFromDataUrl,
  persistAttachmentFromFileUri,
} from "@/attachments/service";
import { collectRetainedAttachmentIds } from "@/attachments/gc-retention";
import { useCreateFlowStore } from "@/stores/create-flow-store";
import { useSessionStore, type SessionState } from "@/stores/session-store";
import { useWorkspaceAttachmentsStore } from "@/attachments/workspace-attachments-store";
import {
  applyClearDraftRecord,
  editDraftRecordText,
  collectQueuedMessageAttachmentIds,
  collectReferencedAttachmentIdsFromState,
  DRAFT_STORE_VERSION,
  DraftStoreStateSchema,
  isAttachmentMetadata,
  isCanonicalDraftInput,
  isLegacyDraftImage,
  normalizeComposerAttachment,
  pruneFinalizedDraftRecords,
  toDraftInputIfReady,
  type DraftInput,
  type DraftLifecycleState,
  type DraftRecord,
  type DraftStoreState,
} from "./state";
import {
  migrateDraftInput,
  migratePersistedState,
  type MigrateLegacyImages,
  PersistedDraftStoreSchema,
} from "./migration";
import { createDraftPersistStorage } from "./persistence";
import { createValidatedPersistStorage } from "@/storage/validated-persist-storage";
import {
  observePersistedState,
  refreshUnchangedPersistedState,
} from "@/storage/sync-persisted-state";

export type { DraftInput, DraftLifecycleState } from "./state";

interface DraftStoreActions {
  getDraftInput: (draftKey: string) => DraftInput | undefined;
  hydrateDraftInput: (input: { draftKey: string }) => Promise<DraftInput | undefined>;
  saveDraftInput: (input: { draftKey: string; draft: DraftInput }) => void;
  editDraftText: (input: { draftKey: string; text: string }) => void;
  clearDraftInput: (input: {
    draftKey: string;
    lifecycle?: Exclude<DraftLifecycleState, "active">;
  }) => void;
  attachWorkspaceFile: (input: {
    draftKey: string;
    attachment: WorkspaceFileComposerAttachment;
  }) => Promise<void>;
  collectActiveAttachmentIds: () => string[];
}

interface DraftStoreRuntimeState {
  attachmentFocusRequestByDraftKey: Record<string, number>;
}

type DraftStore = DraftStoreState & DraftStoreRuntimeState & DraftStoreActions;

let gcScheduled = false;
const sharedDraftStorage = createValidatedPersistStorage(AsyncStorage, PersistedDraftStoreSchema, {
  version: DRAFT_STORE_VERSION,
  mergeRecordFields: ["drafts"],
});
const draftPersistStorage = createDraftPersistStorage(sharedDraftStorage);

const editorPublications = new Map<() => void, () => boolean>();
let editorActivityRevision = 0;
const sharedDraftListeners = new Set<(previous: DraftStoreState, next: DraftStoreState) => void>();

/** Publish DOM-owned input before a checkpoint or a shared refresh can replace its draft. */
export function registerDraftEditorPublication(
  flush: () => void,
  canRefresh: () => boolean = () => true,
): () => void {
  editorPublications.set(flush, canRefresh);
  editorActivityRevision += 1;
  return () => {
    editorPublications.delete(flush);
    editorActivityRevision += 1;
  };
}

/** Only accepted external saves replace an editor's live text; local typing stays DOM-owned. */
export function subscribeToSharedDraftState(
  listener: (previous: DraftStoreState, next: DraftStoreState) => void,
): () => void {
  sharedDraftListeners.add(listener);
  return () => {
    sharedDraftListeners.delete(listener);
  };
}

function publishDraftEditors(): void {
  for (const publish of editorPublications.keys()) publish();
}

async function flushPendingDraftState(): Promise<void> {
  publishDraftEditors();
  await draftPersistStorage.flush();
}

export function flushDraftPersistStorage(): Promise<void> {
  return flushPendingDraftState().catch((error) => {
    console.warn("[DraftStore] Failed to flush draft checkpoint", error);
  });
}

let applySharedDraftState: (state: DraftStoreState) => void;

function captureDraftSetter<Persisted>(
  initializer: StateCreator<DraftStore, [], [["zustand/persist", Persisted]]>,
): StateCreator<DraftStore, [], [["zustand/persist", Persisted]]> {
  return (set, get, api) => {
    // External saves update the view without writing them back as local edits.
    applySharedDraftState = (state) => {
      const previous = get();
      set(state);
      for (const listener of sharedDraftListeners) listener(previous, state);
    };
    return initializer(set, get, api);
  };
}

function createDraftRecord(input: {
  draft: DraftInput;
  lifecycle: DraftLifecycleState;
  previousVersion?: number;
}): DraftRecord {
  return {
    input: {
      text: input.draft.text,
      attachments: input.draft.attachments.map(normalizeComposerAttachment),
    },
    lifecycle: input.lifecycle,
    updatedAt: Date.now(),
    version: (input.previousVersion ?? 0) + 1,
  };
}

const migrateLegacyImages: MigrateLegacyImages = async (images) => {
  if (images.length === 0) {
    return [];
  }

  const migrated = await Promise.all(
    images.map(async (entry) => {
      if (isAttachmentMetadata(entry)) {
        return entry;
      }
      if (!isLegacyDraftImage(entry)) {
        return null;
      }

      try {
        if (entry.uri.startsWith("data:")) {
          return await persistAttachmentFromDataUrl({
            dataUrl: entry.uri,
            mimeType: entry.mimeType,
          });
        }

        return await persistAttachmentFromFileUri({
          uri: entry.uri,
          mimeType: entry.mimeType,
        });
      } catch (error) {
        console.warn("[DraftStore] Failed to migrate legacy draft attachment", {
          uri: entry.uri,
          error,
        });
        return null;
      }
    }),
  );

  return migrated.filter((entry): entry is AttachmentMetadata => entry !== null);
};

async function runAttachmentGc(): Promise<void> {
  gcScheduled = false;
  const nowMs = Date.now();

  useDraftStore.setState((state) => {
    const prunedDrafts = pruneFinalizedDraftRecords({ drafts: state.drafts, nowMs });
    if (prunedDrafts === state.drafts) {
      return state;
    }
    return {
      ...state,
      drafts: prunedDrafts,
    };
  });

  const referencedIds = new Set<string>();
  for (const id of useDraftStore.getState().collectActiveAttachmentIds()) {
    referencedIds.add(id);
  }
  for (const id of collectRetainedAttachmentIds()) {
    referencedIds.add(id);
  }

  const pendingByDraftId = useCreateFlowStore.getState().pendingByDraftId;
  for (const pendingCreate of Object.values(pendingByDraftId)) {
    if (pendingCreate.lifecycle !== "active" || !pendingCreate.images) {
      continue;
    }
    for (const image of pendingCreate.images) {
      referencedIds.add(image.id);
    }
  }

  const sessions = useSessionStore.getState().sessions;
  for (const session of Object.values(sessions)) {
    collectQueuedMessageAttachmentIds(session.queuedMessages, referencedIds);
    collectStreamUserImageIds(session.agentStreamTail, referencedIds);
    collectStreamUserImageIds(session.agentStreamHead, referencedIds);
  }

  // Browser-element screenshots live in the workspace attachment store, not in
  // drafts, so collect their ids here to keep them from being garbage collected
  // before the user sends the message.
  const attachmentsByScope = useWorkspaceAttachmentsStore.getState().attachmentsByScope;
  for (const attachments of Object.values(attachmentsByScope)) {
    for (const attachment of attachments) {
      if (attachment.kind === "browser_element" && attachment.attachment.screenshot) {
        referencedIds.add(attachment.attachment.screenshot.id);
      }
    }
  }

  try {
    await garbageCollectAttachments({ referencedIds });
  } catch (error) {
    console.warn("[DraftStore] Attachment garbage collection failed", error);
  }
}

function collectStreamUserImageIds(
  streams: SessionState["agentStreamTail"],
  referencedIds: Set<string>,
): void {
  for (const stream of streams.values()) {
    for (const item of stream) {
      if (item.kind !== "user_message") continue;
      for (const image of item.images ?? []) {
        referencedIds.add(image.id);
      }
    }
  }
}

function scheduleAttachmentGc(): void {
  if (gcScheduled) {
    return;
  }
  gcScheduled = true;
  if (typeof queueMicrotask === "function") {
    queueMicrotask(() => {
      void runAttachmentGc();
    });
    return;
  }
  setTimeout(() => {
    void runAttachmentGc();
  }, 0);
}

async function migrateAllLegacyDrafts(): Promise<void> {
  const state = useDraftStore.getState();
  const keys = Object.entries(state.drafts)
    .filter(([, record]) => record.lifecycle === "active" && !isCanonicalDraftInput(record.input))
    .map(([draftKey]) => draftKey);

  for (const draftKey of keys) {
    try {
      await state.hydrateDraftInput({ draftKey });
    } catch (error) {
      console.warn("[DraftStore] Failed to migrate draft during startup", {
        draftKey,
        error,
      });
    }
  }
}

export const useDraftStore = create<DraftStore>()(
  captureDraftSetter(
    persist(
      (set, get) => ({
        drafts: {},
        createModalDraft: null,
        attachmentFocusRequestByDraftKey: {},

        getDraftInput: (draftKey) => {
          const record = get().drafts[draftKey];
          return toDraftInputIfReady(record);
        },

        hydrateDraftInput: async ({ draftKey }) => {
          const current = get().drafts[draftKey];
          if (!current) {
            return undefined;
          }
          if (current.lifecycle !== "active") {
            return undefined;
          }
          const ready = toDraftInputIfReady(current);
          if (ready) {
            return ready;
          }

          const migratedDraft = await migrateDraftInput(
            { rawInput: current.input },
            { migrateLegacyImages },
          );

          set((state) => {
            const existing = state.drafts[draftKey];
            if (!existing || existing.version !== current.version) {
              return state;
            }
            return {
              drafts: {
                ...state.drafts,
                [draftKey]: createDraftRecord({
                  draft: migratedDraft,
                  lifecycle: existing.lifecycle,
                  previousVersion: existing.version,
                }),
              },
            };
          });

          scheduleAttachmentGc();
          return migratedDraft;
        },

        saveDraftInput: ({ draftKey, draft }) => {
          set((state) => {
            const existing = state.drafts[draftKey];
            return {
              drafts: {
                ...state.drafts,
                [draftKey]: createDraftRecord({
                  draft,
                  lifecycle: "active",
                  previousVersion: existing?.version,
                }),
              },
            };
          });
          scheduleAttachmentGc();
        },

        editDraftText: ({ draftKey, text }) => {
          set((state) => {
            const previous = state.drafts[draftKey];
            const next = editDraftRecordText(previous, text, Date.now());
            return next === previous ? state : { drafts: { ...state.drafts, [draftKey]: next } };
          });
        },

        clearDraftInput: ({ draftKey, lifecycle }) => {
          set((state) => {
            const existing = state.drafts[draftKey];
            if (!existing) {
              return state;
            }
            const cleared = applyClearDraftRecord({
              record: existing,
              lifecycle,
              nowMs: Date.now(),
            });
            if (cleared) {
              return {
                drafts: {
                  ...state.drafts,
                  [draftKey]: cleared,
                },
              };
            }
            const nextDrafts = { ...state.drafts };
            delete nextDrafts[draftKey];
            return { drafts: nextDrafts };
          });

          scheduleAttachmentGc();
        },

        attachWorkspaceFile: async ({ draftKey, attachment }) => {
          await get().hydrateDraftInput({ draftKey });
          set((state) => {
            const existing = state.drafts[draftKey];
            const draft = toDraftInputIfReady(existing) ?? { text: "", attachments: [] };
            return {
              drafts: {
                ...state.drafts,
                [draftKey]: createDraftRecord({
                  draft: {
                    ...draft,
                    attachments: appendWorkspaceFileAttachment(draft.attachments, attachment),
                  },
                  lifecycle: "active",
                  previousVersion: existing?.version,
                }),
              },
              attachmentFocusRequestByDraftKey: {
                ...state.attachmentFocusRequestByDraftKey,
                [draftKey]: (state.attachmentFocusRequestByDraftKey[draftKey] ?? 0) + 1,
              },
            };
          });
          scheduleAttachmentGc();
        },

        collectActiveAttachmentIds: () => {
          return Array.from(collectReferencedAttachmentIdsFromState(get()).values());
        },
      }),
      {
        name: "paseo-drafts",
        version: DRAFT_STORE_VERSION,
        storage: draftPersistStorage,
        partialize: ({ drafts, createModalDraft }) => ({ drafts, createModalDraft }),
        migrate: (state) =>
          migratePersistedState(state, {
            migrateLegacyImages,
            nowMs: Date.now(),
          }),
        onRehydrateStorage: () => {
          return () => {
            void migrateAllLegacyDrafts();
            scheduleAttachmentGc();
          };
        },
      },
    ),
  ),
);

export async function refreshSharedDraftState(): Promise<boolean> {
  const editorRevision = editorActivityRevision;
  const canRefresh = () => [...editorPublications.values()].every((accept) => accept());
  if (!useDraftStore.persist.hasHydrated() || !canRefresh()) return false;
  return refreshUnchangedPersistedState({
    name: "paseo-drafts",
    storage: sharedDraftStorage,
    revision: useDraftStore.getState,
    flush: flushPendingDraftState,
    beforeAccept: publishDraftEditors,
    accept: (value) => {
      if (editorRevision !== editorActivityRevision || !canRefresh()) return false;
      const result = DraftStoreStateSchema.safeParse(
        value?.state ?? { drafts: {}, createModalDraft: null },
      );
      if (!result.success || (value && value.version !== DRAFT_STORE_VERSION)) {
        throw new Error("Cannot refresh incompatible draft state");
      }
      applySharedDraftState(result.data);
      return true;
    },
  });
}

/** App-shell lifecycle owns browser listeners, including the unpublished IME candidate guard. */
export function observeSharedDraftState(): () => void {
  if (typeof window === "undefined" || typeof document === "undefined") return () => {};
  const composing = new Set<Node>();
  const stopCompositionGuard = registerDraftEditorPublication(
    () => {},
    () => {
      for (const node of composing) if (!node.isConnected) composing.delete(node);
      return composing.size === 0;
    },
  );
  const compositionChanged = (event: Event) => {
    if (!(event.target instanceof Node)) return;
    if (event.type === "compositionstart") composing.add(event.target);
    else composing.delete(event.target);
    editorActivityRevision += 1;
  };
  // Composition candidates are still DOM-owned and intentionally absent from onChangeText.
  document.addEventListener("compositionstart", compositionChanged, true);
  document.addEventListener("compositionend", compositionChanged, true);
  const stop = observePersistedState({ name: "paseo-drafts", refresh: refreshSharedDraftState });
  return () => {
    stop();
    stopCompositionGuard();
    document.removeEventListener("compositionstart", compositionChanged, true);
    document.removeEventListener("compositionend", compositionChanged, true);
  };
}

import AsyncStorage from "@react-native-async-storage/async-storage";
import { create } from "zustand";
import { persist, type StateStorage } from "zustand/middleware";
import { z } from "zod";
import { createValidatedPersistStorage } from "@/storage/validated-persist-storage";

// Persist only ended request identities: no question, answer or draft text.
const schema = z.strictObject({ ended: z.array(z.tuple([z.string(), z.string()])).max(256) });
type SavedState = z.infer<typeof schema>;

interface BtwVisibility extends SavedState {
  hydrated: boolean;
  finishHydration(): void;
  /** Hide the BTW panel for this host+session identity until a new request reveals it. */
  end(owner: string, reveal: string): void;
}

export function createBtwVisibilityStore(storage: StateStorage) {
  return create<BtwVisibility>()(
    persist<BtwVisibility, [], [], SavedState>(
      (set) => ({
        ended: [],
        hydrated: false,
        finishHydration: () => set({ hydrated: true }),
        end: (owner, reveal) =>
          set((state) => ({
            ended: [
              ...state.ended.filter(([key]) => key !== owner),
              [owner, reveal] as [string, string],
            ].slice(-256),
          })),
      }),
      {
        name: "btw-visibility",
        storage: createValidatedPersistStorage(storage, schema),
        partialize: ({ ended }) => ({ ended }),
        onRehydrateStorage: (state) => () => state.finishHydration(),
      },
    ),
  );
}

export const useBtwVisibilityStore = createBtwVisibilityStore(AsyncStorage);

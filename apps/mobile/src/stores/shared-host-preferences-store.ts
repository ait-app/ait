import AsyncStorage from "@react-native-async-storage/async-storage";
import { create } from "zustand";
import { persist, type StateStorage } from "zustand/middleware";
import { z } from "zod";
import { createValidatedPersistStorage } from "@/storage/validated-persist-storage";

export interface SharedHostAccount {
  center: string;
  accountId?: string;
}

const preferencesSchema = z.strictObject({ disabledHostKeys: z.array(z.string()) });
type PersistedPreferences = z.infer<typeof preferencesSchema>;

interface SharedHostPreferences extends PersistedPreferences {
  hydrated: boolean;
  finishHydration(): void;
  setHostEnabled(account: SharedHostAccount, hostId: string, enabled: boolean): void;
}

function hostPreferenceKey(account: SharedHostAccount, hostId: string): string {
  return JSON.stringify([account.center.replace(/\/+$/, ""), account.accountId ?? "", hostId]);
}

export function isSharedHostEnabled(
  preferences: PersistedPreferences,
  account: SharedHostAccount,
  hostId: string,
): boolean {
  return !preferences.disabledHostKeys.includes(hostPreferenceKey(account, hostId));
}

export function createSharedHostPreferencesStore(storage: StateStorage) {
  return create<SharedHostPreferences>()(
    persist<SharedHostPreferences, [], [], PersistedPreferences>(
      (set) => ({
        disabledHostKeys: [],
        hydrated: false,
        finishHydration: () => set({ hydrated: true }),
        setHostEnabled: (account, hostId, enabled) =>
          set((state) => {
            const key = hostPreferenceKey(account, hostId);
            if (enabled === !state.disabledHostKeys.includes(key)) return state;
            return {
              disabledHostKeys: enabled
                ? state.disabledHostKeys.filter((candidate) => candidate !== key)
                : [...state.disabledHostKeys, key],
            };
          }),
      }),
      {
        name: "shared-host-preferences",
        storage: createValidatedPersistStorage(storage, preferencesSchema),
        partialize: ({ disabledHostKeys }) => ({ disabledHostKeys }),
        onRehydrateStorage: (state) => () => state.finishHydration(),
      },
    ),
  );
}

export const useSharedHostPreferences = createSharedHostPreferencesStore(AsyncStorage);

import AsyncStorage from "@react-native-async-storage/async-storage";
import { create } from "zustand";
import { persist } from "zustand/middleware";
import { z } from "zod";
import { createValidatedPersistStorage } from "@/storage/validated-persist-storage";

const ProjectPinsSchema = z.strictObject({ pinnedProjectKeys: z.array(z.string()) });

interface SidebarProjectPinState {
  pinnedProjectKeys: string[];
  toggleProjectPin: (key: string) => void;
}

export const useSidebarProjectPinStore = create<SidebarProjectPinState>()(
  persist(
    (set) => ({
      pinnedProjectKeys: [],
      toggleProjectPin: (rawKey) => {
        const key = rawKey.trim();
        if (!key) return;
        set((state) => ({
          pinnedProjectKeys: state.pinnedProjectKeys.includes(key)
            ? state.pinnedProjectKeys.filter((value) => value !== key)
            : [key, ...state.pinnedProjectKeys],
        }));
      },
    }),
    {
      name: "sidebar-project-pins",
      storage: createValidatedPersistStorage(AsyncStorage, ProjectPinsSchema),
      partialize: (state) => ({ pinnedProjectKeys: state.pinnedProjectKeys }),
    },
  ),
);

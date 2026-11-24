import AsyncStorage from "@react-native-async-storage/async-storage";
import { create } from "zustand";
import { persist } from "zustand/middleware";
import { z } from "zod";
import type { CheckoutPrMergeMethod } from "@ait/protocol/messages";
import { createValidatedPersistStorage } from "@/storage/validated-persist-storage";

const MergeMethodsSchema = z.strictObject({
  methods: z.record(z.string(), z.enum(["squash", "merge", "rebase"])),
});

interface MergeMethodState {
  methods: Record<string, CheckoutPrMergeMethod>;
  setMethod: (key: string, method: CheckoutPrMergeMethod) => void;
}

/** The merge method is remembered per host and project; the key keeps the two parts apart. */
export function mergeMethodKey(serverId: string, projectId: string): string {
  return JSON.stringify([serverId, projectId]);
}

export const useMergeMethodStore = create<MergeMethodState>()(
  persist(
    (set) => ({
      methods: {},
      setMethod: (key, method) => {
        set((state) => ({ methods: { ...state.methods, [key]: method } }));
      },
    }),
    {
      name: "git-merge-method",
      storage: createValidatedPersistStorage(AsyncStorage, MergeMethodsSchema, {
        mergeRecordFields: ["methods"],
      }),
      partialize: (state) => ({ methods: state.methods }),
    },
  ),
);

import { requireOptionalNativeModule } from "expo-modules-core";
import type { SubscriptionStore, StoreTransaction } from "./store";

type StoreModule = Omit<SubscriptionStore, "subscribe"> & {
  addListener(
    event: "transaction",
    listener: (transaction: StoreTransaction) => void,
  ): { remove(): void };
};
const native = requireOptionalNativeModule<StoreModule>("AitStoreKit");
export const subscriptionStore: SubscriptionStore | null = native
  ? {
      product: () => native.product(),
      purchase: (token) => native.purchase(token),
      transactions: () => native.transactions(),
      restore: () => native.restore(),
      finish: (id) => native.finish(id),
      subscribe(listener) {
        const subscription = native.addListener("transaction", listener);
        return () => subscription.remove();
      },
    }
  : null;

import { requireOptionalNativeModule } from "expo-modules-core";
import type { PurchaseResult, StoreProduct, StoreTransaction, SubscriptionStore } from "./store";
interface PlayModule {
  product(productId: string, basePlanId: string): Promise<StoreProduct | null>;
  purchase(productId: string, basePlanId: string, accountToken: string): Promise<PurchaseResult>;
  transactions(productId: string): Promise<StoreTransaction[]>;
  addListener(
    event: "transaction",
    listener: (transaction: StoreTransaction) => void,
  ): { remove(): void };
}
const native = requireOptionalNativeModule<PlayModule>("AitPlayBilling");
const productId = process.env.EXPO_PUBLIC_GOOGLE_PLAY_PRODUCT_ID ?? "monthly";
const basePlanId = process.env.EXPO_PUBLIC_GOOGLE_PLAY_BASE_PLAN_ID ?? "monthly";
export const subscriptionStore: SubscriptionStore | null = native
  ? {
      provider: "google_play",
      productId,
      basePlanId,
      product: () => native.product(productId, basePlanId),
      purchase: (token) => native.purchase(productId, basePlanId, token),
      transactions: () => native.transactions(productId),
      // Play restoration queries owned purchases. Acknowledgement happens on the secure server.
      restore: async () => undefined,
      finish: async () => undefined,
      subscribe(listener) {
        const subscription = native.addListener("transaction", listener);
        return () => subscription.remove();
      },
    }
  : null;

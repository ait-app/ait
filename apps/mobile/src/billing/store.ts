export interface StoreTransaction {
  id: string;
  productId: string;
  environment: string;
}
export interface StoreProduct {
  id: string;
  displayPrice: string;
}
export type PurchaseResult =
  | { status: "purchased"; transaction: StoreTransaction }
  | { status: "pending" | "cancelled" };

export interface SubscriptionStore {
  product(): Promise<StoreProduct | null>;
  purchase(accountToken: string): Promise<PurchaseResult>;
  transactions(): Promise<StoreTransaction[]>;
  restore(): Promise<void>;
  finish(id: string): Promise<void>;
  subscribe(listener: (transaction: StoreTransaction) => void): () => void;
}

/** Other platforms can read membership; only iOS offers App Store purchasing. */
export const subscriptionStore: SubscriptionStore | null = null;

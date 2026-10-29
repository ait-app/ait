export type StoreProvider = "app_store" | "google_play";
export type StoreTransaction =
  | { store?: "app_store"; id: string; productId: string; environment: string }
  | { store: "google_play"; id: string; productId: string; purchaseToken: string };
export interface StoreProduct {
  id: string;
  displayPrice: string;
}
export type PurchaseResult =
  | { status: "purchased"; transaction: StoreTransaction }
  | { status: "pending" | "cancelled" };
export interface SubscriptionStore {
  readonly provider: StoreProvider;
  readonly productId: string;
  readonly basePlanId?: string;
  product(): Promise<StoreProduct | null>;
  purchase(accountToken: string): Promise<PurchaseResult>;
  transactions(includeHistory?: boolean): Promise<StoreTransaction[]>;
  restore(): Promise<void>;
  finish(id: string): Promise<void>;
  subscribe(listener: (transaction: StoreTransaction) => void): () => void;
}
/** Desktop and web can read membership; native stores deliver payments. */
export const subscriptionStore: SubscriptionStore | null = null;

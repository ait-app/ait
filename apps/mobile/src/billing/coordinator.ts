import type {
  AppStoreTransaction,
  OnlineServiceMembership,
} from "@ait/client/internal/account-session";
import type { SubscriptionStore, StoreTransaction } from "./store";

interface BillingAccount {
  identity(): string | null;
  membership(): Promise<OnlineServiceMembership>;
  verify(transaction: AppStoreTransaction): Promise<OnlineServiceMembership>;
  verifyPlay(purchase: { purchase_token: string }): Promise<OnlineServiceMembership>;
  updated(membership: OnlineServiceMembership): void;
}

/** Apple delivers at least once. Finish only after this account's server persisted the purchase. */
export class PurchaseCoordinator {
  private inFlight = new Map<string, Promise<void>>();
  constructor(
    private store: SubscriptionStore,
    private account: BillingAccount,
  ) {}

  async purchase(): Promise<"purchased" | "pending" | "cancelled"> {
    const identity = this.requireAccount();
    const membership = await this.account.membership();
    this.assertAccount(identity);
    const option = purchaseOption(membership, this.store);
    if (
      !option.available ||
      option.product_id !== this.store.productId ||
      (this.store.provider === "google_play" && option.base_plan_id !== this.store.basePlanId)
    )
      throw new Error("Purchases are unavailable for this service.");
    const result = await this.store.purchase(membership.app_account_token);
    this.assertAccount(identity);
    if (result.status === "purchased") await this.accept(result.transaction, identity);
    return result.status;
  }

  async synchronize(restore = false): Promise<number> {
    const identity = this.requireAccount();
    if (restore) await this.store.restore();
    this.assertAccount(identity);
    const transactions = await this.store.transactions(restore);
    this.assertAccount(identity);
    const results = await Promise.allSettled(
      transactions.map((transaction) => this.accept(transaction, identity)),
    );
    const failed = results.find((result) => result.status === "rejected");
    if (failed?.status === "rejected") throw failed.reason;
    const membership = await this.account.membership();
    this.assertAccount(identity);
    this.account.updated(membership);
    return transactions.length;
  }

  accept(transaction: StoreTransaction, identity = this.requireAccount()): Promise<void> {
    const key = `${identity}:${transaction.id}`;
    const existing = this.inFlight.get(key);
    if (existing) return existing;
    const operation = this.verifyAndFinish(transaction, identity).finally(() =>
      this.inFlight.delete(key),
    );
    this.inFlight.set(key, operation);
    return operation;
  }

  private async verifyAndFinish(transaction: StoreTransaction, identity: string): Promise<void> {
    this.assertAccount(identity);
    if (transaction.productId !== this.store.productId)
      throw new Error("Unsupported subscription product.");
    if (transaction.store === "google_play") {
      if (
        this.store.provider !== "google_play" ||
        !transaction.purchaseToken ||
        transaction.purchaseToken.length > 4096
      )
        throw new Error("Invalid Google Play purchase.");
      const membership = await this.account.verifyPlay({
        purchase_token: transaction.purchaseToken,
      });
      this.assertAccount(identity);
      this.account.updated(membership);
      await this.store.finish(transaction.id);
      return;
    }
    if (
      this.store.provider !== "app_store" ||
      !/^\d{1,64}$/.test(transaction.id) ||
      (transaction.environment !== "Production" && transaction.environment !== "Sandbox")
    )
      throw new Error("Unsupported purchase. Use an App Store sandbox account for testing.");
    const membership = await this.account.verify({
      transaction_id: transaction.id,
      environment: transaction.environment,
    });
    this.assertAccount(identity);
    this.account.updated(membership);
    await this.store.finish(transaction.id);
  }
  private requireAccount(): string {
    const identity = this.account.identity();
    if (!identity) throw new Error("Sign in to the Ait account used for this purchase.");
    return identity;
  }
  private assertAccount(identity: string): void {
    if (this.account.identity() !== identity)
      throw new Error("The account changed. Sign in again to restore the purchase.");
  }
}

export function purchaseOption(
  membership: OnlineServiceMembership,
  store: Pick<SubscriptionStore, "provider">,
) {
  return (
    membership.purchase_options?.[store.provider] ?? {
      available: store.provider === "app_store" && membership.purchases_available,
      product_id: store.provider === "app_store" ? membership.product_id : "monthly",
      base_plan_id: null,
    }
  );
}

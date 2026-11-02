import { describe, expect, it, vi } from "vitest";
import { PurchaseCoordinator } from "./coordinator";
import type { SubscriptionStore, StoreTransaction } from "./store";
import type { OnlineServiceMembership } from "@ait/client/internal/account-session";

const membership: OnlineServiceMembership = {
  required: true,
  purchases_available: true,
  app_account_token: "018f20b0-fbcc-7000-8000-000000000001",
  product_id: "monthly",
  can_connect: true,
  status: "active",
  expires_at: "2030-01-01T00:00:00Z",
};
const transaction: StoreTransaction = {
  id: "20000001",
  productId: "monthly",
  environment: "Sandbox",
};
function fixture() {
  let identity: string | null = "account-a";
  const store = {
    provider: "app_store" as const,
    productId: "monthly",
    product: vi.fn().mockResolvedValue({ id: "monthly", displayPrice: "$4.99" }),
    purchase: vi
      .fn<SubscriptionStore["purchase"]>()
      .mockResolvedValue({ status: "purchased", transaction }),
    transactions: vi.fn().mockResolvedValue([transaction]),
    restore: vi.fn().mockResolvedValue(undefined),
    finish: vi.fn().mockResolvedValue(undefined),
    subscribe: vi.fn().mockReturnValue(() => undefined),
  };
  const account = {
    identity: () => identity,
    membership: vi.fn().mockResolvedValue(membership),
    verify: vi.fn().mockResolvedValue(membership),
    verifyPlay: vi.fn().mockResolvedValue(membership),
    updated: vi.fn(),
  };
  return {
    store,
    account,
    coordinator: new PurchaseCoordinator(store, account),
    change: (value: string | null) => {
      identity = value;
    },
  };
}
describe("account-bound StoreKit purchase delivery", () => {
  it("binds purchases and verifies before finishing without trusting client expiry", async () => {
    const f = fixture();
    f.account.verify.mockImplementation(async () => {
      expect(f.store.finish).not.toHaveBeenCalled();
      return membership;
    });
    expect(await f.coordinator.purchase()).toBe("purchased");
    expect(f.store.purchase).toHaveBeenCalledWith(membership.app_account_token);
    expect(f.account.verify).toHaveBeenCalledWith({
      transaction_id: transaction.id,
      environment: "Sandbox",
    });
    expect(f.store.finish).toHaveBeenCalledWith(transaction.id);
  });
  it.each(["pending", "cancelled"] as const)(
    "keeps %s separate from a granted membership",
    async (status) => {
      const f = fixture();
      f.store.purchase.mockResolvedValue({ status });
      expect(await f.coordinator.purchase()).toBe(status);
      expect(f.account.verify).not.toHaveBeenCalled();
      expect(f.store.finish).not.toHaveBeenCalled();
    },
  );
  it("leaves an interrupted verification unfinished and retries it on restore", async () => {
    const f = fixture();
    f.account.verify.mockRejectedValueOnce(new Error("offline"));
    await expect(f.coordinator.purchase()).rejects.toThrow("offline");
    expect(f.store.finish).not.toHaveBeenCalled();
    expect(await f.coordinator.synchronize(true)).toBe(1);
    expect(f.store.restore).toHaveBeenCalledOnce();
    expect(f.store.finish).toHaveBeenCalledOnce();
  });
  it("coalesces update events and purchase results for the same transaction", async () => {
    const f = fixture();
    await Promise.all([f.coordinator.accept(transaction), f.coordinator.accept(transaction)]);
    expect(f.account.verify).toHaveBeenCalledOnce();
    expect(f.store.finish).toHaveBeenCalledOnce();
  });
  it("discards a delayed response when the signed-in account changes", async () => {
    const f = fixture();
    f.account.verify.mockImplementation(async () => {
      f.change("account-b");
      return membership;
    });
    await expect(f.coordinator.accept(transaction)).rejects.toThrow("account changed");
    expect(f.account.updated).not.toHaveBeenCalled();
    expect(f.store.finish).not.toHaveBeenCalled();
  });
  it("requires login, rejects local StoreKit configurations, and preserves ownership errors", async () => {
    const f = fixture();
    f.change(null);
    await expect(f.coordinator.purchase()).rejects.toThrow("Sign in");
    expect(f.store.purchase).not.toHaveBeenCalled();
    f.change("account-a");
    await expect(f.coordinator.accept({ ...transaction, environment: "Xcode" })).rejects.toThrow(
      "sandbox",
    );
    f.account.verify.mockRejectedValue(new Error("purchase belongs to another Ait account"));
    await expect(f.coordinator.synchronize(true)).rejects.toThrow("another Ait account");
    expect(f.store.finish).not.toHaveBeenCalled();
  });
});

it("delivers Play tokens to the Play verifier while the same account also has Apple membership", async () => {
  const f = fixture();
  const store: SubscriptionStore = {
    ...f.store,
    provider: "google_play",
    productId: "monthly",
    basePlanId: "monthly",
  };
  const playTransaction: StoreTransaction = {
    store: "google_play",
    id: "token-hash",
    productId: "monthly",
    purchaseToken: "private-play-fixture",
  };
  f.store.purchase.mockResolvedValue({ status: "purchased", transaction: playTransaction });
  f.account.membership.mockResolvedValue({
    ...membership,
    purchase_options: {
      app_store: { available: true, product_id: "monthly", base_plan_id: null },
      google_play: { available: true, product_id: "monthly", base_plan_id: "monthly" },
    },
    subscriptions: [
      {
        store: "app_store",
        status: "active",
        expires_at: "2030-01-01T00:00:00Z",
        auto_renewing: true,
      },
    ],
  });
  const coordinator = new PurchaseCoordinator(store, f.account);
  expect(await coordinator.purchase()).toBe("purchased");
  expect(f.account.verifyPlay).toHaveBeenCalledWith({ purchase_token: "private-play-fixture" });
  expect(f.account.verify).not.toHaveBeenCalled();
  expect(f.store.finish).toHaveBeenCalledWith("token-hash");
});

it("rejects Play product/base-plan drift and leaves failed Play verification retryable", async () => {
  const f = fixture();
  const store: SubscriptionStore = {
    ...f.store,
    provider: "google_play",
    productId: "monthly",
    basePlanId: "monthly",
  };
  const coordinator = new PurchaseCoordinator(store, f.account);
  await expect(coordinator.purchase()).rejects.toThrow("unavailable");
  expect(f.store.purchase).not.toHaveBeenCalled();
  const transaction: StoreTransaction = {
    store: "google_play",
    id: "token-hash",
    productId: "monthly",
    purchaseToken: "private-play-fixture",
  };
  f.account.verifyPlay.mockRejectedValue(new Error("verification offline"));
  await expect(coordinator.accept(transaction)).rejects.toThrow("offline");
  expect(f.store.finish).not.toHaveBeenCalled();
});

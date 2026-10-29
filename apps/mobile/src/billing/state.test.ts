import { beforeEach, expect, it, vi } from "vitest";
import {
  observePurchases,
  purchaseMembership,
  refreshMembership,
  resetBillingState,
  useBillingState,
} from "./state";
import type { OnlineServiceMembership } from "@ait/client/internal/account-session";
import type { StoreTransaction } from "./store";
const mocks = vi.hoisted(() => ({
  account: { accountId: "account-a", status: "online", center: "https://aitapp.cn/api" },
  membership: vi.fn(),
  verify: vi.fn(),
  verifyPlay: vi.fn(),
  refresh: vi.fn(),
  store: {
    provider: "app_store",
    productId: "monthly",
    product: vi.fn(),
    purchase: vi.fn(),
    transactions: vi.fn(),
    restore: vi.fn(),
    finish: vi.fn(),
    subscribe: vi.fn(),
  },
}));
vi.mock("@/runtime/account-state", () => ({
  useAccountState: { getState: () => mocks.account },
  loadOnlineServiceMembership: mocks.membership,
  verifyAppStoreTransaction: mocks.verify,
  verifyGooglePlayPurchase: mocks.verifyPlay,
  accountCommand: mocks.refresh,
}));
vi.mock("./store", () => ({ subscriptionStore: mocks.store }));
const inactive: OnlineServiceMembership = {
  required: true,
  purchases_available: true,
  app_account_token: "018f20b0-fbcc-7000-8000-000000000001",
  product_id: "monthly",
  can_connect: false,
  status: "inactive",
  expires_at: null,
};
const active: OnlineServiceMembership = {
  ...inactive,
  can_connect: true,
  status: "active",
  expires_at: "2030-01-01T00:00:00Z",
};
const transaction = { id: "20000001", productId: "monthly", environment: "Sandbox" };
beforeEach(() => {
  vi.clearAllMocks();
  resetBillingState();
  mocks.account.accountId = "account-a";
  mocks.account.center = "https://aitapp.cn/api";
  mocks.membership.mockResolvedValue(inactive);
  mocks.verify.mockResolvedValue(active);
  mocks.store.product.mockResolvedValue({ id: "monthly", displayPrice: "$4.99" });
  mocks.store.purchase.mockResolvedValue({ status: "purchased", transaction });
  mocks.store.finish.mockResolvedValue(undefined);
  mocks.refresh.mockResolvedValue(undefined);
});
it("keeps a verified purchase when an older membership refresh arrives late", async () => {
  let finish!: (membership: OnlineServiceMembership) => void;
  mocks.membership.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const refresh = refreshMembership();
  await purchaseMembership();
  expect(useBillingState.getState().membership?.status).toBe("active");
  finish(inactive);
  await refresh;
  expect(useBillingState.getState().membership?.status).toBe("active");
  expect(mocks.refresh).toHaveBeenCalledWith("account_refresh");
});
it("discards membership from a previous account and offers no purchase on an untrusted center", async () => {
  let finish!: (membership: OnlineServiceMembership) => void;
  mocks.membership.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const refresh = refreshMembership();
  mocks.account.accountId = "account-b";
  resetBillingState();
  finish(active);
  await refresh;
  expect(useBillingState.getState().membership).toBeNull();
  mocks.account.center = "https://custom.test/api";
  await purchaseMembership();
  expect(mocks.store.purchase).not.toHaveBeenCalled();
  expect(useBillingState.getState().error).toContain("unavailable");
});
it("delivers a deferred approval outside the purchase screen and clears its pending notice", async () => {
  let receive!: (transaction: StoreTransaction) => void;
  const remove = vi.fn();
  mocks.store.subscribe.mockImplementationOnce((listener) => {
    receive = listener;
    return remove;
  });
  const stop = observePurchases();
  mocks.store.purchase.mockResolvedValueOnce({ status: "pending" });
  await purchaseMembership();
  expect(useBillingState.getState().message).toBe("pending");
  receive(transaction);
  await vi.waitFor(() => expect(mocks.store.finish).toHaveBeenCalledWith(transaction.id));
  expect(useBillingState.getState().membership?.can_connect).toBe(true);
  expect(useBillingState.getState().message).toBeNull();
  stop();
  expect(remove).toHaveBeenCalledOnce();
});

it("shows pending approval when the other store already provides active membership", async () => {
  const otherStore: OnlineServiceMembership = {
    ...active,
    subscriptions: [
      {
        store: "google_play",
        status: "active",
        auto_renewing: true,
        expires_at: "2030-01-01T00:00:00Z",
      },
    ],
  };
  mocks.membership.mockResolvedValue(otherStore);
  await refreshMembership();
  mocks.store.purchase.mockResolvedValueOnce({ status: "pending" });
  await purchaseMembership();
  expect(useBillingState.getState().message).toBe("pending");
  expect(useBillingState.getState().membership?.can_connect).toBe(true);
});

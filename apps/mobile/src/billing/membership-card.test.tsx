/** @vitest-environment jsdom */
import React from "react";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { OnlineServiceMembershipCard } from "./membership-card";
const mocks = vi.hoisted(() => ({
  state: {
    membership: {
      required: true,
      purchases_available: true,
      can_connect: false,
      product_id: "monthly",
      app_account_token: "token",
      status: "inactive",
      expires_at: null as string | null,
    },
    price: "¥38.00" as string | null,
    busy: false,
    error: null as string | null,
    message: null as string | null,
  },
  store: {
    provider: "app_store" as "app_store" | "google_play",
    productId: "monthly",
    basePlanId: "monthly",
  },
  purchase: vi.fn(),
  restore: vi.fn(),
  refresh: vi.fn(),
  open: vi.fn(),
}));
vi.mock("./store", () => ({ subscriptionStore: mocks.store }));
vi.mock("./state", () => ({
  useBillingState: () => mocks.state,
  supportsStorePurchases: () => true,
  purchaseMembership: mocks.purchase,
  restoreMembership: mocks.restore,
  refreshMembership: mocks.refresh,
}));
vi.mock("@/utils/open-external-url", () => ({ openExternalUrl: mocks.open }));
beforeEach(async () => {
  vi.clearAllMocks();
  mocks.store.provider = "app_store";
  Object.assign(mocks.state.membership, { subscriptions: undefined, purchase_options: undefined });
  mocks.state.membership.status = "inactive";
  mocks.state.membership.expires_at = null;
  mocks.state.price = "¥38.00";
  mocks.state.busy = false;
  mocks.state.message = null;
  mocks.state.error = null;
  mocks.open.mockResolvedValue(undefined);
  await i18n.changeLanguage("en");
});
afterEach(cleanup);
it("offers the localized monthly price, restore, benefit and automatic renewal disclosure", () => {
  const view = render(<OnlineServiceMembershipCard />);
  expect(view.getByText("Subscribe · ¥38.00 / month")).toBeTruthy();
  expect(view.getByText("Connect to your computers using Ait Online Service.")).toBeTruthy();
  expect(view.getByText(/renews automatically each month/)).toBeTruthy();
  fireEvent.click(view.getByTestId("subscribe-online-service"));
  expect(mocks.purchase).toHaveBeenCalledOnce();
  fireEvent.click(view.getByTestId("restore-online-service"));
  expect(mocks.restore).toHaveBeenCalledOnce();
  fireEvent.click(view.getByText("Privacy Policy"));
  expect(mocks.open).toHaveBeenCalledWith("https://ait-app.com/privacy/");
});
it("keeps restoration available for expired members and hides repurchase for active members", () => {
  mocks.state.membership.status = "expired";
  mocks.state.membership.expires_at = "2030-01-01T00:00:00Z";
  const view = render(<OnlineServiceMembershipCard />);
  expect(view.getByText("Expired")).toBeTruthy();
  expect(view.getByTestId("restore-online-service")).toBeTruthy();
  mocks.state.membership.status = "active";
  view.rerender(<OnlineServiceMembershipCard />);
  expect(view.queryByTestId("subscribe-online-service")).toBeNull();
  expect(view.getByText("Active")).toBeTruthy();
});
it("disables buying before product loading completes or while a purchase is pending", () => {
  mocks.state.price = null;
  const view = render(<OnlineServiceMembershipCard />);
  fireEvent.click(view.getByTestId("subscribe-online-service"));
  expect(mocks.purchase).not.toHaveBeenCalled();
  mocks.state.price = "$4.99";
  mocks.state.busy = true;
  mocks.state.message = "pending";
  view.rerender(<OnlineServiceMembershipCard />);
  fireEvent.click(view.getByTestId("subscribe-online-service"));
  expect(mocks.purchase).not.toHaveBeenCalled();
  expect(view.getByText(/awaiting Apple approval/)).toBeTruthy();
});

it("allows the other store to add time while showing both subscriptions once purchased", () => {
  mocks.state.membership.status = "active";
  Object.assign(mocks.state.membership, {
    subscriptions: [
      {
        store: "google_play",
        status: "active",
        expires_at: "2030-01-01T00:00:00Z",
        auto_renewing: true,
      },
    ],
  });
  const view = render(<OnlineServiceMembershipCard />);
  expect(view.getByTestId("subscribe-online-service")).toBeTruthy();
  expect(view.getByText(/their paid time adds up/)).toBeTruthy();
  Object.assign(mocks.state.membership, {
    subscriptions: [
      {
        store: "google_play",
        status: "active",
        expires_at: "2030-01-01T00:00:00Z",
        auto_renewing: true,
      },
      {
        store: "app_store",
        status: "active",
        expires_at: "2030-01-01T00:00:00Z",
        auto_renewing: true,
      },
    ],
  });
  view.rerender(<OnlineServiceMembershipCard />);
  expect(view.queryByTestId("subscribe-online-service")).toBeNull();
  expect(view.getByText("Google Play subscription")).toBeTruthy();
  expect(view.getByText("App Store subscription")).toBeTruthy();
});
it("uses Google Play renewal and management on Android even with active Apple credit", () => {
  mocks.store.provider = "google_play";
  mocks.state.membership.status = "active";
  Object.assign(mocks.state.membership, {
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
  const view = render(<OnlineServiceMembershipCard />);
  expect(view.getByTestId("subscribe-online-service")).toBeTruthy();
  expect(view.getByText(/Payment is charged through Google Play/)).toBeTruthy();
  fireEvent.click(view.getByText("Manage subscription"));
  expect(mocks.open).toHaveBeenCalledWith(
    "https://play.google.com/store/account/subscriptions?sku=monthly&package=dev.ait.mobile",
  );
});

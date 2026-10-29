import { create } from "zustand";
import {
  DEFAULT_ACCOUNT_CENTER,
  type OnlineServiceMembership,
} from "@ait/client/internal/account-session";
import {
  accountCommand,
  loadOnlineServiceMembership,
  useAccountState,
  verifyAppStoreTransaction,
} from "@/runtime/account-state";
import { subscriptionStore } from "./store";
import { PurchaseCoordinator } from "./coordinator";

interface BillingState {
  membership: OnlineServiceMembership | null;
  price: string | null;
  busy: boolean;
  error: string | null;
  message: "pending" | "restored" | "nothingToRestore" | null;
}
export const useBillingState = create<BillingState>(() => ({
  membership: null,
  price: null,
  busy: false,
  error: null,
  message: null,
}));

/** Store purchases are offered only on the account authority selected by this build. */
export function supportsAppStorePurchases(): boolean {
  const center = process.env.EXPO_PUBLIC_APP_STORE_CENTER ?? DEFAULT_ACCOUNT_CENTER;
  return (
    subscriptionStore !== null &&
    useAccountState.getState().center.replace(/\/$/, "") === center.replace(/\/$/, "")
  );
}
export function billingIdentity(): string | null {
  const account = useAccountState.getState();
  return account.status !== "logged_out" && account.accountId
    ? JSON.stringify([account.center, account.accountId])
    : null;
}
const coordinator = subscriptionStore
  ? new PurchaseCoordinator(subscriptionStore, {
      identity: billingIdentity,
      membership: loadOnlineServiceMembership,
      verify: verifyAppStoreTransaction,
      updated: (membership) => {
        const reconnect =
          membership.can_connect && !useBillingState.getState().membership?.can_connect;
        ++membershipRevision;
        useBillingState.setState({
          membership,
          error: null,
          message:
            membership.status === "active" || membership.status === "grace_period"
              ? null
              : useBillingState.getState().message,
        });
        if (reconnect) void accountCommand("account_refresh").catch(() => undefined);
      },
    })
  : null;

let epoch = 0;
let membershipRevision = 0;
export function resetBillingState(): void {
  ++epoch;
  ++membershipRevision;
  useBillingState.setState({
    membership: null,
    price: null,
    busy: false,
    error: null,
    message: null,
  });
}

export async function refreshMembership(): Promise<void> {
  const revision = membershipRevision;
  const identity = billingIdentity();
  const generation = epoch;
  if (!identity) return;
  try {
    const [membership, product] = await Promise.all([
      loadOnlineServiceMembership(),
      supportsAppStorePurchases() ? subscriptionStore!.product().catch(() => null) : null,
    ]);
    if (generation === epoch && revision === membershipRevision && billingIdentity() === identity)
      useBillingState.setState({ membership, price: product?.displayPrice ?? null, error: null });
  } catch (error) {
    if (generation === epoch && revision === membershipRevision && billingIdentity() === identity)
      useBillingState.setState({ error: message(error) });
  }
}

export async function purchaseMembership(): Promise<void> {
  await perform(async () => {
    if (!supportsAppStorePurchases() || !coordinator)
      throw new Error("App Store purchases are unavailable for this service.");
    const result = await coordinator.purchase();
    if (result === "pending" && useBillingState.getState().membership?.status !== "active")
      useBillingState.setState({ message: "pending" });
  });
}
export async function restoreMembership(): Promise<void> {
  await perform(async () => {
    if (!supportsAppStorePurchases() || !coordinator)
      throw new Error("App Store purchases are unavailable for this service.");
    const count = await coordinator.synchronize(true);
    useBillingState.setState({ message: count ? "restored" : "nothingToRestore" });
  });
}
async function perform(work: () => Promise<void>): Promise<void> {
  if (useBillingState.getState().busy) return;
  const generation = epoch;
  useBillingState.setState({ busy: true, error: null, message: null });
  try {
    await work();
  } catch (error) {
    if (generation === epoch) useBillingState.setState({ error: message(error) });
  } finally {
    if (generation === epoch) useBillingState.setState({ busy: false });
  }
}
export async function reconcilePurchases(): Promise<void> {
  if (!supportsAppStorePurchases() || !coordinator || !billingIdentity()) return;
  const generation = epoch;
  try {
    await coordinator.synchronize();
  } catch (error) {
    if (generation === epoch) useBillingState.setState({ error: message(error) });
  }
}
export function observePurchases(): () => void {
  return (
    subscriptionStore?.subscribe((transaction) => {
      if (!supportsAppStorePurchases() || !coordinator || !billingIdentity()) return;
      const generation = epoch;
      void coordinator.accept(transaction).catch((error) => {
        if (generation === epoch) useBillingState.setState({ error: message(error) });
      });
    }) ?? (() => undefined)
  );
}
function message(error: unknown): string {
  return error instanceof Error ? error.message : "Purchase verification failed. Try again.";
}

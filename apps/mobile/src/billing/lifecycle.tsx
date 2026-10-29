import { useEffect } from "react";
import { useAppVisible } from "@/hooks/use-app-visible";
import { useAccountState } from "@/runtime/account-state";
import {
  observePurchases,
  reconcilePurchases,
  refreshMembership,
  resetBillingState,
} from "./state";

/** One observer handles pending purchases even when the purchase screen is closed. */
export function OnlineServiceBillingLifecycle() {
  const account = useAccountState();
  const identity =
    account.status !== "logged_out" && account.accountId
      ? JSON.stringify([account.center, account.accountId])
      : null;
  const visible = useAppVisible();
  useEffect(() => observePurchases(), []);
  useEffect(() => {
    resetBillingState();
  }, [identity]);
  useEffect(() => {
    if (!identity || !visible) return;
    let disposed = false;
    const refresh = async () => {
      await refreshMembership();
      if (!disposed) await reconcilePurchases();
    };
    void refresh();
    const timer = setInterval(() => void refreshMembership(), 60_000);
    return () => {
      disposed = true;
      clearInterval(timer);
    };
  }, [identity, visible]);
  return null;
}

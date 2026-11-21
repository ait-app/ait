import { useEffect, useState } from "react";
import { useAppVisible } from "@/hooks/use-app-visible";
import { loadAccountInfo, useAccountState } from "@/runtime/account-state";

const HOUR_MS = 60 * 60 * 1000;
const DAY_MS = 24 * HOUR_MS;
/** Longest delay a timer accepts before it fires immediately. */
const MAX_TIMER_MS = 2 ** 31 - 1;
const REFRESH_INTERVAL_MS = 10 * 60 * 1000;

/**
 * What is known about when the account runs out. Far-off expiries only name their scale; the
 * last week is counted, in whole days and then whole hours.
 */
export type AccountExpiry =
  | { kind: "unknown" | "never" | "expired" | "months" | "weeks" }
  | { kind: "days" | "hours"; count: number };

const MONTHS_FROM_MS = 30 * DAY_MS;
const WEEKS_FROM_MS = 7 * DAY_MS;
const DAYS_FROM_MS = 2 * DAY_MS;

export interface ResolvedAccountExpiry {
  expiry: AccountExpiry;
  /** When the expiry next reads differently, or null when it never will. */
  changesAt: number | null;
}

/** `undefined` means the expiry is not known yet; `null` means the account does not expire. */
export function resolveAccountExpiry(
  expiresAt: string | null | undefined,
  now: number,
): ResolvedAccountExpiry {
  if (expiresAt === null) return { expiry: { kind: "never" }, changesAt: null };
  const expiresAtMs = expiresAt === undefined ? Number.NaN : Date.parse(expiresAt);
  if (!Number.isFinite(expiresAtMs)) return { expiry: { kind: "unknown" }, changesAt: null };

  const remaining = expiresAtMs - now;
  if (remaining <= 0) return { expiry: { kind: "expired" }, changesAt: null };
  // Each reading holds until just after the time left drops below its lower bound.
  if (remaining >= MONTHS_FROM_MS)
    return { expiry: { kind: "months" }, changesAt: expiresAtMs - MONTHS_FROM_MS + 1 };
  if (remaining >= WEEKS_FROM_MS)
    return { expiry: { kind: "weeks" }, changesAt: expiresAtMs - WEEKS_FROM_MS + 1 };
  if (remaining >= DAYS_FROM_MS) {
    const days = Math.floor(remaining / DAY_MS);
    return { expiry: { kind: "days", count: days }, changesAt: expiresAtMs - days * DAY_MS + 1 };
  }
  const hours = Math.floor(remaining / HOUR_MS);
  return {
    // The last hour still reads as one hour; only a passed expiry reads as expired.
    expiry: { kind: "hours", count: Math.max(hours, 1) },
    changesAt: hours === 0 ? expiresAtMs : expiresAtMs - hours * HOUR_MS + 1,
  };
}

/**
 * The signed-in account's expiry, kept current as time passes.
 *
 * The account snapshot only records the expiry seen at sign-in, so a renewal made since then is
 * read from the service while the app is visible; the snapshot covers the time before the first
 * answer and any failed read.
 */
export function useAccountExpiry(): AccountExpiry {
  const online = useAccountState((state) => state.status === "online");
  const accountKey = useAccountState((state) =>
    JSON.stringify([state.center, state.accountId, state.name]),
  );
  const snapshotExpiresAt = useAccountState((state) => state.accountExpiresAt);
  const visible = useAppVisible();
  const [loaded, setLoaded] = useState<{ key: string; expiresAt: string | null } | null>(null);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!visible || !online) return;
    let disposed = false;
    const refresh = async () => {
      try {
        const info = await loadAccountInfo();
        if (!disposed) setLoaded({ key: accountKey, expiresAt: info.expiresAt });
      } catch {
        // Keep the last known expiry; the next refresh tries again.
      }
    };
    void refresh();
    const timer = setInterval(() => void refresh(), REFRESH_INTERVAL_MS);
    return () => {
      disposed = true;
      clearInterval(timer);
    };
  }, [visible, online, accountKey]);

  const expiresAt = loaded?.key === accountKey ? loaded.expiresAt : snapshotExpiresAt;
  const { expiry, changesAt } = resolveAccountExpiry(expiresAt, now);

  useEffect(() => {
    if (changesAt === null) return;
    // A delay past the timer limit wakes early; the reading is unchanged and the timer re-arms.
    const delay = Math.min(Math.max(changesAt - Date.now(), 0), MAX_TIMER_MS);
    const timer = setTimeout(() => setNow(Date.now()), delay);
    return () => clearTimeout(timer);
  }, [changesAt, now]);

  // Returning to the app after a long sleep can skip a timer; re-read the clock when shown again.
  useEffect(() => {
    if (visible) setNow(Date.now());
  }, [visible]);

  return expiry;
}

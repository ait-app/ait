import { normalizeCenter } from "@ait/client/internal/account-session";

/** Resolve a saved host's non-secret service binding before requesting a relay ticket. */
export function parseAccountRelayTarget(url: string): { hostId: string; center: string } {
  const target = new URL(url);
  const centers = target.searchParams.getAll("center");
  if (
    target.protocol !== "ait+desktop:" ||
    target.hostname !== "account-relay" ||
    !/^\/[0-9a-f-]{36}$/i.test(target.pathname) ||
    target.port ||
    target.hash ||
    target.username ||
    target.password ||
    centers.length !== 1 ||
    !centers[0]?.trim() ||
    [...target.searchParams.keys()].some((key) => key !== "center")
  ) {
    throw new Error("Invalid account relay target.");
  }
  return { hostId: target.pathname.slice(1), center: normalizeCenter(centers[0]) };
}

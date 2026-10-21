import type { AccountSessionManager, AccountSnapshot } from "@ait/client/internal/account-session";
import type { HubLink } from "@ait/client/internal/hub";

/** Native mobile platforms supply an implementation using secure system credential storage. */
export async function getNativeAccount(): Promise<AccountSessionManager> {
  throw new Error("Account login is available in the native mobile and desktop apps.");
}

export function subscribeNativeAccount(_listener: (state: AccountSnapshot) => void): () => void {
  return () => {};
}

export function registerNativeAccountTransport(_close: () => void): () => void {
  throw new Error("Native account relay is unavailable on this platform.");
}

/** Native mobile platforms own one hub link per signed-in account. */
export function getNativeHubLink(): HubLink | Promise<HubLink | null> | null {
  return null;
}

import type { AccountSessionManager, AccountSnapshot } from "@ait/client/internal/account-session";

/** Android supplies an implementation using its encrypted system credential storage. */
export async function getNativeAccount(): Promise<AccountSessionManager> {
  throw new Error("Account login is available in the Android and desktop apps.");
}

export function subscribeNativeAccount(_listener: (state: AccountSnapshot) => void): () => void {
  return () => {};
}

export function registerNativeAccountTransport(_close: () => void): () => void {
  throw new Error("Native account relay is unavailable on this platform.");
}

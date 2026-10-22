import type { AccountSessionManager, AccountSnapshot } from "@ait/client/internal/account-session";
import type { HubLink } from "@ait/client/internal/hub";

export function getBrowserHubLink(): HubLink | null {
  return null;
}

export function supportsBrowserAccount(): boolean {
  return false;
}
export async function getBrowserAccount(): Promise<AccountSessionManager> {
  throw new Error("Browser account sessions are unavailable on this platform.");
}
export function subscribeBrowserAccount(_listener: (state: AccountSnapshot) => void): () => void {
  return () => {};
}
export function registerBrowserAccountTransport(_close: () => void): () => void {
  throw new Error("Browser account sessions are unavailable on this platform.");
}

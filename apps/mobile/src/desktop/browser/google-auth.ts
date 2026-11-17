/**
 * Google account origins reject authentication inside an embedded user agent.
 * Ordinary pages on www.google.com remain available in the workspace browser.
 */
export function isGoogleAuthenticationUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return url.protocol === "https:" && url.hostname === "accounts.google.com";
  } catch {
    return false;
  }
}

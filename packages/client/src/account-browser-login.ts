export interface AccountBrowserLogin {
  randomSecret(): string;
  challenge(verifier: string): Promise<string>;
  open(
    authorizationUrl: (redirectUri: string) => string,
    state: string,
    signal: AbortSignal,
  ): Promise<{ url: string; redirectUri: string }>;
}

/** Accept only the callback and state created by this in-memory login attempt. */
export function parseAccountCallback(url: string, redirectUri: string, state: string): URL | null {
  try {
    const callback = new URL(url);
    const expected = new URL(redirectUri);
    if (
      callback.protocol !== expected.protocol ||
      callback.host !== expected.host ||
      callback.pathname !== expected.pathname ||
      callback.username ||
      callback.password ||
      callback.hash ||
      callback.searchParams.getAll("state").length !== 1 ||
      callback.searchParams.get("state") !== state
    )
      return null;
    const codes = callback.searchParams.getAll("code");
    const errors = callback.searchParams.getAll("error");
    if (codes.length === 1 && /^[a-f0-9]{64}$/.test(codes[0]!) && !errors.length) return callback;
    if (errors.length === 1 && /^[a-z_]{1,80}$/.test(errors[0]!) && !codes.length) return callback;
    return null;
  } catch {
    return null;
  }
}

export function accountLoginError(code: string): string {
  switch (code) {
    case "account_expired":
      return "Your account has expired. Contact your administrator to renew access.";
    case "contact_verification_required":
      return "Bind and verify your phone number or email in the account center, then sign in again.";
    case "email_verification_required":
      return "Bind and verify your email in the account center, then sign in again.";
    case "account_link_required":
      return "This email has an existing AIT account. Sign in to the web console with its original password and link unified login in Settings.";
    case "authing_cancelled":
      return "Sign-in cancelled.";
    case "authing_unavailable":
      return "The sign-in service is temporarily unavailable. Try again later.";
    default:
      return "Sign-in did not complete. Start a new sign-in from AIT.";
  }
}

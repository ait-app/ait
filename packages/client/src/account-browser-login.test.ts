import { describe, expect, it } from "vitest";
import { accountLoginError, parseAccountCallback } from "./account-browser-login.js";

const state = "a".repeat(64);
const code = "b".repeat(64);
describe("native account callbacks", () => {
  it.each(["ait://auth/callback", "http://127.0.0.1:42345/auth/callback"])(
    "accepts the exact %s callback and state",
    (target) => {
      expect(
        parseAccountCallback(
          `${target}?code=${code}&state=${state}`,
          target,
          state,
        )?.searchParams.get("code"),
      ).toBe(code);
      expect(
        parseAccountCallback(
          `${target}?error=account_expired&state=${state}`,
          target,
          state,
        )?.searchParams.get("error"),
      ).toBe("account_expired");
    },
  );
  it.each([
    `ait://evil/callback?code=${code}&state=${state}`,
    `ait://auth/other?code=${code}&state=${state}`,
    `ait://user@auth/callback?code=${code}&state=${state}`,
    `ait://auth/callback?code=${code}&state=wrong`,
    `ait://auth/callback?code=${code}&state=${state}&state=${state}`,
    `ait://auth/callback?code=${code}&code=${code}&state=${state}`,
    `ait://auth/callback?code=${code}&error=cancelled&state=${state}`,
    `ait://auth/callback?code=jwt&state=${state}`,
    `ait://auth/callback?code=${code}&state=${state}#fragment`,
    "not a URL",
  ])("rejects foreign or ambiguous callbacks", (url) => {
    expect(parseAccountCallback(url, "ait://auth/callback", state)).toBeNull();
  });
  it("provides actionable verification, linking and expiration errors", () => {
    expect(accountLoginError("account_expired")).toContain("renew");
    expect(accountLoginError("account_link_required")).toContain("original password");
    expect(accountLoginError("email_verification_required")).toContain("verify");
  });
});

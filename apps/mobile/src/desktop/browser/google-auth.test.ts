import { describe, expect, it } from "vitest";
import { isGoogleAuthenticationUrl } from "./google-auth";

describe("isGoogleAuthenticationUrl", () => {
  it("recognizes Google account sign-in URLs", () => {
    expect(
      isGoogleAuthenticationUrl(
        "https://accounts.google.com/ServiceLogin?hl=zh-CN&continue=https://www.google.com/",
      ),
    ).toBe(true);
  });

  it.each([
    "https://www.google.com/",
    "https://accounts.google.com.evil.example/ServiceLogin",
    "http://accounts.google.com/ServiceLogin",
    "not a URL",
  ])("leaves %s in the workspace browser", (url) => {
    expect(isGoogleAuthenticationUrl(url)).toBe(false);
  });
});

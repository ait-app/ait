import { describe, expect, it } from "vitest";
import { desktopBrowserLogin } from "./account-browser-login.js";

describe("desktop system browser login", () => {
  it("binds loopback and ignores foreign callbacks until the expected state arrives", async () => {
    const state = "a".repeat(64);
    const abort = new AbortController();
    const driver = desktopBrowserLogin(async (target) => {
      const redirect = new URL(new URL(target).searchParams.get("redirect_uri")!);
      expect(redirect.hostname).toBe("127.0.0.1");
      expect(Number(redirect.port)).toBeGreaterThanOrEqual(1024);
      expect((await fetch(`${redirect}?state=wrong&code=${"b".repeat(64)}`)).status).toBe(400);
      expect((await fetch(`${redirect}?state=${state}&code=${"b".repeat(64)}`)).status).toBe(200);
    });
    const callback = await driver.open(
      (uri) => `https://center.test/v1/auth/authorize?redirect_uri=${encodeURIComponent(uri)}`,
      state,
      abort.signal,
    );
    expect(callback.url).toContain(`state=${state}`);
    await expect(fetch(callback.redirectUri)).rejects.toThrow();
  });
  it("cleans up the listener when cancelled or the browser fails to open", async () => {
    const abort = new AbortController();
    let redirect = "";
    const driver = desktopBrowserLogin(async () => {
      abort.abort();
    });
    await expect(
      driver.open(
        (uri) => {
          redirect = uri;
          return "https://center.test";
        },
        "a".repeat(64),
        abort.signal,
      ),
    ).rejects.toThrow("cancelled");
    await expect(fetch(redirect)).rejects.toThrow();
    const failed = desktopBrowserLogin(async () => {
      throw new Error("OS unavailable");
    });
    await expect(
      failed.open(() => "https://center.test", "a".repeat(64), new AbortController().signal),
    ).rejects.toThrow("system browser");
  });
});

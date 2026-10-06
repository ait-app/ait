import { beforeEach, describe, expect, it, vi } from "vitest";
import { androidBrowserLogin } from "./account-browser-login.native";
const mocks = vi.hoisted(() => ({
  listener: undefined as undefined | ((event: { url: string }) => void),
  remove: vi.fn(),
  open: vi.fn(async () => {}),
}));
vi.mock("react-native", () => ({
  Linking: {
    openURL: mocks.open,
    addEventListener: (_event: string, listener: (event: { url: string }) => void) => {
      mocks.listener = listener;
      return { remove: mocks.remove };
    },
  },
}));
vi.mock("expo-crypto", () => ({}));
beforeEach(() => {
  vi.clearAllMocks();
});
describe("Android system browser login", () => {
  it("registers the callback before opening and ignores another login's state", async () => {
    const state = "a".repeat(64);
    const pending = androidBrowserLogin.open(
      (uri) => `https://center.test/?redirect=${uri}`,
      state,
      new AbortController().signal,
    );
    expect(mocks.listener).toBeDefined();
    expect(mocks.open).toHaveBeenCalledOnce();
    mocks.listener!({ url: `ait://auth/callback?state=wrong&code=${"b".repeat(64)}` });
    expect(mocks.remove).not.toHaveBeenCalled();
    mocks.listener!({ url: `ait://auth/callback?state=${state}&code=${"b".repeat(64)}` });
    expect((await pending).redirectUri).toBe("ait://auth/callback");
    expect(mocks.remove).toHaveBeenCalledOnce();
  });
  it("removes the URL subscription when the user cancels", async () => {
    const abort = new AbortController();
    const pending = androidBrowserLogin.open(
      () => "https://center.test",
      "a".repeat(64),
      abort.signal,
    );
    abort.abort();
    await expect(pending).rejects.toThrow("cancelled");
    expect(mocks.remove).toHaveBeenCalledOnce();
  });
});

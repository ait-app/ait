import { beforeEach, describe, expect, it, vi } from "vitest";
import { androidBrowserLogin, iosBrowserLogin } from "./account-browser-login.native";
const mocks = vi.hoisted(() => ({
  listener: undefined as undefined | ((event: { url: string }) => void),
  remove: vi.fn(),
  authSession: vi.fn(),
  dismiss: vi.fn(),
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
vi.mock("expo-web-browser", () => ({
  openAuthSessionAsync: mocks.authSession,
  dismissAuthSession: mocks.dismiss,
}));
beforeEach(() => {
  vi.resetAllMocks();
  mocks.listener = undefined;
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

describe("iOS system authentication session", () => {
  const state = "a".repeat(64);
  const redirectUri = "ait://auth/callback";
  const callback = `${redirectUri}?state=${state}&code=${"b".repeat(64)}`;
  const authorize = (uri: string) =>
    `https://center.test/authorize?redirect_uri=${encodeURIComponent(uri)}`;

  it.each([callback, `${redirectUri}?state=${state}&error=access_denied`])(
    "returns a matching callback to the account authority: %s",
    async (url) => {
      mocks.authSession.mockResolvedValue({ type: "success", url });
      const abort = new AbortController();
      await expect(iosBrowserLogin.open(authorize, state, abort.signal)).resolves.toEqual({
        url,
        redirectUri,
      });
      expect(mocks.authSession).toHaveBeenCalledExactlyOnceWith(
        authorize(redirectUri),
        redirectUri,
      );
      expect(mocks.listener).toBeUndefined();
      expect(mocks.open).not.toHaveBeenCalled();
      abort.abort();
      expect(mocks.dismiss).not.toHaveBeenCalled();
    },
  );

  it.each([
    callback.replace(state, "wrong-state"),
    callback.replace("auth/callback", "other/callback"),
    callback.replace("ait:", "https:"),
    `${callback}&state=${state}`,
    `${callback}&error=access_denied`,
  ])("rejects an unrelated or ambiguous callback: %s", async (url) => {
    mocks.authSession.mockResolvedValue({ type: "success", url });
    await expect(
      iosBrowserLogin.open(authorize, state, new AbortController().signal),
    ).rejects.toThrow("callback is invalid");
  });

  it.each(["cancel", "dismiss"])("handles native %s and permits a retry", async (type) => {
    mocks.authSession
      .mockResolvedValueOnce({ type })
      .mockResolvedValueOnce({ type: "success", url: callback });
    await expect(
      iosBrowserLogin.open(authorize, state, new AbortController().signal),
    ).rejects.toThrow("cancelled");
    await expect(
      iosBrowserLogin.open(authorize, state, new AbortController().signal),
    ).resolves.toEqual({ url: callback, redirectUri });
    expect(mocks.dismiss).not.toHaveBeenCalled();
  });

  it("dismisses on abort and ignores a late callback while a new attempt is open", async () => {
    let complete!: (result: { type: string; url: string }) => void;
    mocks.authSession.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        }),
    );
    const abort = new AbortController();
    const pending = iosBrowserLogin.open(authorize, state, abort.signal);
    abort.abort();
    await expect(pending).rejects.toThrow("cancelled");
    expect(mocks.dismiss).toHaveBeenCalledOnce();
    mocks.authSession.mockResolvedValueOnce({ type: "success", url: callback });
    const retry = iosBrowserLogin.open(authorize, state, new AbortController().signal);
    complete({ type: "success", url: callback });
    await expect(retry).resolves.toEqual({ url: callback, redirectUri });
    expect(mocks.dismiss).toHaveBeenCalledOnce();
  });

  it("does not open or dismiss any session for a previously cancelled attempt", async () => {
    const abort = new AbortController();
    abort.abort();
    await expect(iosBrowserLogin.open(authorize, state, abort.signal)).rejects.toThrow("cancelled");
    expect(mocks.authSession).not.toHaveBeenCalled();
    expect(mocks.dismiss).not.toHaveBeenCalled();
  });

  it("still cancels if dismissing the native sheet fails", async () => {
    mocks.authSession.mockImplementationOnce(() => new Promise(() => {}));
    mocks.dismiss.mockImplementationOnce(() => {
      throw new Error("already closed");
    });
    const abort = new AbortController();
    const pending = iosBrowserLogin.open(authorize, state, abort.signal);
    abort.abort();
    await expect(pending).rejects.toThrow("cancelled");
  });

  it("reports native launch failures without leaking the authorization URL", async () => {
    mocks.authSession.mockRejectedValueOnce(new Error("native error with private URL"));
    const abort = new AbortController();
    await expect(iosBrowserLogin.open(authorize, state, abort.signal)).rejects.toThrow(
      "Could not open the system browser.",
    );
    abort.abort();
    expect(mocks.dismiss).not.toHaveBeenCalled();
  });
});

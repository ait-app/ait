import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  set: vi.fn(async () => {}),
  remove: vi.fn(async () => {}),
  getInstallation: vi.fn(async () => "11111111-1111-4111-8111-111111111111"),
  setInstallation: vi.fn(async () => {}),
  appState: vi.fn(),
}));
vi.mock("expo-secure-store", () => ({
  getItemAsync: mocks.get,
  setItemAsync: mocks.set,
  deleteItemAsync: mocks.remove,
}));
vi.mock("@react-native-async-storage/async-storage", () => ({
  default: { getItem: mocks.getInstallation, setItem: mocks.setInstallation },
}));
vi.mock("expo-crypto", () => ({ randomUUID: () => "22222222-2222-4222-8222-222222222222" }));
vi.mock("expo/fetch", () => ({ fetch: (...args: Parameters<typeof fetch>) => fetch(...args) }));
vi.mock("react-native", () => ({
  AppState: { currentState: "active", addEventListener: mocks.appState },
}));
vi.mock("@/utils/app-version", () => ({ resolveAppVersion: () => "0.0.14" }));

beforeEach(() => {
  vi.resetModules();
  vi.clearAllMocks();
  vi.useFakeTimers();
  mocks.get.mockResolvedValue(null);
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("Android account storage and lifecycle", () => {
  it("persists only the session in SecureStore, clears it on logout, and creates one lifecycle listener", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string) =>
        url.endsWith("/auth/login")
          ? Response.json({
              access_token: "private-jwt",
              expires_in: 3600,
              user: { email: "me@example.test" },
            })
          : new Response(null, { status: 204 }),
      ),
    );
    const native = await import("./native-account.android");
    const manager = await native.getNativeAccount();
    expect(await native.getNativeAccount()).toBe(manager);
    await manager.login("", "me@example.test", "private-password");
    expect(mocks.set).toHaveBeenCalledWith(
      "ait.account.session.v1",
      expect.stringContaining("private-jwt"),
    );
    expect(JSON.stringify(mocks.set.mock.calls)).not.toContain("private-password");
    expect(mocks.setInstallation).not.toHaveBeenCalled();
    expect(mocks.appState).toHaveBeenCalledOnce();
    const close = vi.fn();
    native.registerNativeAccountTransport(close);
    const onState = mocks.appState.mock.calls[0]![1];
    onState("background");
    expect(close).toHaveBeenCalledOnce();
    await manager.logout();
    expect(mocks.remove).toHaveBeenLastCalledWith("ait.account.session.v1");
  });

  it.each([
    "bad-json",
    JSON.stringify({ token: "expired", expiresAt: 0 }),
    JSON.stringify({
      token: "secret",
      center: "invalid-url",
      name: "Me",
      expiresAt: Date.now() + 60000,
    }),
  ])("discards corrupt or expired credentials without authenticating: %s", async (saved) => {
    mocks.get.mockResolvedValue(saved);
    const fetch = vi.fn();
    vi.stubGlobal("fetch", fetch);
    const { getNativeAccount } = await import("./native-account.android");
    expect((await getNativeAccount()).snapshot().status).toBe("logged_out");
    expect(mocks.remove).toHaveBeenCalledWith("ait.account.session.v1");
    expect(fetch).not.toHaveBeenCalled();
  });
});

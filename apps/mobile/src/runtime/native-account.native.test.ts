import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  set: vi.fn(async () => {}),
  remove: vi.fn(async () => {}),
  getInstallation: vi.fn(async () => "11111111-1111-4111-8111-111111111111"),
  setInstallation: vi.fn(async () => {}),
  appState: vi.fn(),
  authSession: vi.fn(),
  dismissAuthSession: vi.fn(),
  platform: "android",
}));
vi.mock("expo-secure-store", () => ({
  getItemAsync: mocks.get,
  setItemAsync: mocks.set,
  deleteItemAsync: mocks.remove,
}));
vi.mock("@react-native-async-storage/async-storage", () => ({
  default: { getItem: mocks.getInstallation, setItem: mocks.setInstallation },
}));
vi.mock("expo-crypto", () => ({
  randomUUID: () => "22222222-2222-4222-8222-222222222222",
  getRandomValues: (bytes: Uint8Array) => bytes.fill(0xab),
  digestStringAsync: vi.fn(async () => "test+challenge/value="),
  CryptoDigestAlgorithm: { SHA256: "SHA-256" },
  CryptoEncoding: { BASE64: "base64" },
}));
vi.mock("expo-web-browser", () => ({
  openAuthSessionAsync: mocks.authSession,
  dismissAuthSession: mocks.dismissAuthSession,
}));
vi.mock("expo/fetch", () => ({ fetch: (...args: Parameters<typeof fetch>) => fetch(...args) }));
vi.mock("react-native", () => ({
  AppState: { currentState: "active", addEventListener: mocks.appState },
  Platform: {
    get OS() {
      return mocks.platform;
    },
  },
}));
vi.mock("@/utils/app-version", () => ({ resolveAppVersion: () => "0.0.14" }));

beforeEach(() => {
  vi.resetModules();
  vi.clearAllMocks();
  vi.useFakeTimers();
  mocks.platform = "android";
  mocks.get.mockResolvedValue(null);
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("native mobile account storage and lifecycle", () => {
  it("completes iOS hosted login after backgrounding and stores only the exchanged AIT session", async () => {
    mocks.platform = "ios";
    const code = "c".repeat(64);
    const expiresAt = new Date(Date.now() + 7 * 86400_000).toISOString();
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string) => {
        if (url.endsWith("/v1/auth/providers"))
          return Response.json({ authing_enabled: true, native_login_enabled: true });
        if (url.endsWith("/v1/auth/client/exchange"))
          return Response.json({
            access_token: "ait-session-token",
            expires_in: 3600,
            user: { email: "me@example.test", display_name: "Me", expires_at: expiresAt },
          });
        if (url.endsWith("/v1/nodes/register"))
          return Response.json({
            node_id: "node",
            node_session_id: "session",
            host_id: null,
            control_required: false,
          });
        return Response.json({ hosts: [], count: 0 });
      }),
    );
    const { getNativeAccount } = await import("./native-account.native");
    const manager = await getNativeAccount();
    const onState = mocks.appState.mock.calls[0]![1];
    mocks.authSession.mockImplementationOnce(async (url: string, redirectUri: string) => {
      const authorization = new URL(url);
      expect(authorization.origin).toBe("https://center.test");
      expect(authorization.pathname).toBe("/api/v1/auth/authorize");
      expect(authorization.searchParams.get("redirect_uri")).toBe("ait://auth/callback");
      expect(authorization.searchParams.get("code_challenge")).toBe("test-challenge_value");
      onState("inactive");
      onState("background");
      expect(manager.snapshot().loginPending).toBe(true);
      return {
        type: "success",
        url: `${redirectUri}?state=${authorization.searchParams.get("state")}&code=${code}`,
      };
    });
    const snapshot = await manager.loginWithBrowser("https://center.test/api");
    expect(snapshot).toMatchObject({
      name: "Me",
      accountExpiresAt: expiresAt,
      loginPending: false,
    });
    const exchange = vi
      .mocked(globalThis.fetch)
      .mock.calls.find(([url]) => String(url).endsWith("/v1/auth/client/exchange"));
    expect(JSON.parse(String(exchange?.[1]?.body))).toEqual({
      code,
      code_verifier: "ab".repeat(32),
    });
    expect(mocks.set).toHaveBeenCalledWith(
      "ait.account.session.v1",
      expect.stringContaining("ait-session-token"),
    );
    expect(JSON.stringify(mocks.set.mock.calls)).not.toContain(code);
    expect(JSON.stringify(mocks.set.mock.calls)).not.toContain("ab".repeat(32));
    expect(mocks.dismissAuthSession).not.toHaveBeenCalled();
    onState("active");
    await vi.advanceTimersByTimeAsync(0);
    const registration = vi
      .mocked(globalThis.fetch)
      .mock.calls.find(([url]) => String(url).endsWith("/v1/nodes/register"));
    expect(JSON.parse(String(registration?.[1]?.body))).toMatchObject({
      platform: "ios",
      display_name: "Ait iOS",
    });
    await manager.logout();
  });
  it.each(["android", "ios"])(
    "persists the %s session and closes transports in the background",
    async (platform) => {
      mocks.platform = platform;
      vi.stubGlobal(
        "fetch",
        vi.fn(async (url: string) => {
          if (url.endsWith("/auth/login"))
            return Response.json({
              access_token: "private-jwt",
              expires_in: 3600,
              user: { email: "me@example.test" },
            });
          if (url.endsWith("/v1/nodes/register"))
            return Response.json({
              node_id: "node",
              node_session_id: "session",
              host_id: null,
              control_required: false,
            });
          return Response.json({ hosts: [], count: 0 });
        }),
      );
      const native = await import("./native-account.native");
      const manager = await native.getNativeAccount();
      expect(await native.getNativeAccount()).toBe(manager);
      await manager.login("", "me@example.test", "private-password");
      await vi.advanceTimersByTimeAsync(0);
      const registration = vi
        .mocked(globalThis.fetch)
        .mock.calls.find(([url]) => String(url).endsWith("/v1/nodes/register"));
      expect(JSON.parse(String(registration?.[1]?.body))).toMatchObject({
        display_name: platform === "ios" ? "Ait iOS" : "Ait Android",
        platform,
        runtime: null,
      });
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
    },
  );

  it.each(["android", "ios"])(
    "restores a saved %s session after a process restart and retries an offline startup",
    async (platform) => {
      mocks.platform = platform;
      const saved = JSON.stringify({
        center: "https://center.test/api",
        token: "saved-jwt",
        expiresAt: Date.now() + 3_600_000,
        name: "Me",
        nodeSessionId: "previous-session",
      });
      mocks.get.mockResolvedValue(saved);
      let offline = true;
      const http = vi.fn(async (url: string) => {
        if (offline) throw new TypeError("Network request failed");
        if (url.endsWith("/v1/nodes/register"))
          return Response.json({
            node_id: "node",
            node_session_id: "new-session",
            host_id: null,
            control_required: false,
          });
        return Response.json({ hosts: [], count: 0 });
      });
      vi.stubGlobal("fetch", http);
      const { getNativeAccount } = await import("./native-account.native");
      const manager = await getNativeAccount();
      await vi.advanceTimersByTimeAsync(1);
      expect(manager.snapshot()).toMatchObject({ status: "error", name: "Me" });
      expect(mocks.remove).not.toHaveBeenCalled();
      expect(http.mock.calls[0]?.[0]).toBe(
        "https://center.test/api/v1/node-sessions/previous-session",
      );

      offline = false;
      await vi.advanceTimersByTimeAsync(6_000);
      expect(manager.snapshot()).toMatchObject({ status: "online", name: "Me" });
      expect(http.mock.calls.some(([url]) => url.includes("/auth/"))).toBe(false);
      expect(mocks.remove).not.toHaveBeenCalled();
      expect(mocks.set).toHaveBeenCalledWith(
        "ait.account.session.v1",
        expect.stringContaining('"nodeSessionId":"new-session"'),
      );
      await manager.shutdown();
      expect(mocks.remove).not.toHaveBeenCalled();
    },
  );

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
    const { getNativeAccount } = await import("./native-account.native");
    expect((await getNativeAccount()).snapshot().status).toBe("logged_out");
    expect(mocks.remove).toHaveBeenCalledWith("ait.account.session.v1");
    expect(fetch).not.toHaveBeenCalled();
  });
});

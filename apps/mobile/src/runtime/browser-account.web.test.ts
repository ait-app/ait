// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
vi.mock("@/utils/app-version", () => ({ resolveAppVersion: () => "test" }));
const token = () =>
  `header.${btoa(JSON.stringify({ exp: Math.floor(Date.now() / 1000) + 3600 }))}.signature`;
let manager: import("@ait/client/internal/account-session").AccountSessionManager | undefined;
let request: ReturnType<typeof vi.fn>;

beforeEach(() => {
  vi.resetModules();
  vi.useFakeTimers();
  localStorage.clear();
  sessionStorage.clear();
  vi.stubEnv("EXPO_PUBLIC_AIT_CONSOLE", "1");
  request = vi.fn(async (url: string) => {
    if (url.endsWith("/v1/me")) return Response.json({ email: "web@example.test" });
    if (url.endsWith("/v1/nodes/register"))
      return Response.json({
        node_id: "node",
        node_session_id: "session",
        host_id: null,
        control_required: false,
      });
    if (url.includes("/v1/hosts/online")) return Response.json({ hosts: [], count: 0 });
    return new Response(null, { status: 204 });
  });
  vi.stubGlobal("fetch", request);
});
afterEach(async () => {
  await manager?.shutdown();
  manager = undefined;
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.unstubAllEnvs();
});

describe("browser console account", () => {
  it("reuses the website login and registers a client-only node with the same-origin service", async () => {
    const saved = token();
    localStorage.setItem("ait.access-token", saved);
    const { getBrowserAccount } = await import("./browser-account.web");
    manager = await getBrowserAccount();
    await vi.advanceTimersByTimeAsync(1);
    expect(manager.snapshot()).toMatchObject({
      status: "online",
      center: `${location.origin}/api`,
      name: "web@example.test",
      hostOnline: false,
    });
    const registration = request.mock.calls.find(([url]) => url.endsWith("/v1/nodes/register"));
    expect(JSON.parse(registration![1].body)).toMatchObject({ runtime: null, platform: "web" });
    expect(registration![1].headers.Authorization).toBe(`Bearer ${saved}`);
    expect(localStorage.getItem("ait.access-token")).toBe(saved);
  });
  it("does not register or discover before sign-in", async () => {
    manager = await (await import("./browser-account.web")).getBrowserAccount();
    expect(manager.snapshot()).toMatchObject({
      status: "logged_out",
      center: `${location.origin}/api`,
    });
    expect(request).not.toHaveBeenCalled();
    await expect(manager.loginWithBrowser("https://another.test/api")).rejects.toThrow(
      "this website",
    );
  });
  it("clears a rejected credential without attempting node registration", async () => {
    localStorage.setItem("ait.access-token", token());
    request.mockResolvedValueOnce(new Response(null, { status: 401 }));
    manager = await (await import("./browser-account.web")).getBrowserAccount();
    expect(manager.snapshot().status).toBe("logged_out");
    expect(localStorage.getItem("ait.access-token")).toBeNull();
    expect(request).toHaveBeenCalledTimes(1);
  });
  it("preserves credentials on network failure and allows retry", async () => {
    const saved = token();
    localStorage.setItem("ait.access-token", saved);
    request.mockRejectedValueOnce(new TypeError("offline"));
    const { getBrowserAccount } = await import("./browser-account.web");
    await expect(getBrowserAccount()).rejects.toThrow("offline");
    expect(localStorage.getItem("ait.access-token")).toBe(saved);
    manager = await getBrowserAccount();
    expect(manager.snapshot().status).toBe("connecting");
  });
  it("an old tab logout closes its transports but cannot erase a newer login", async () => {
    const saved = token();
    localStorage.setItem("ait.access-token", saved);
    const { getBrowserAccount, registerBrowserAccountTransport } =
      await import("./browser-account.web");
    manager = await getBrowserAccount();
    const close = vi.fn();
    registerBrowserAccountTransport(close);
    localStorage.setItem("ait.access-token", "newer-login");
    await manager.logout();
    expect(close).toHaveBeenCalled();
    expect(localStorage.getItem("ait.access-token")).toBe("newer-login");
    expect(request).toHaveBeenCalledWith(
      `${location.origin}/api/v1/auth/logout`,
      expect.objectContaining({ headers: { Authorization: `Bearer ${saved}` } }),
    );
  });
});

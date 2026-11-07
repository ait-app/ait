import { centerLink } from "../../../../packages/client/test/hub";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  AccountSessionManager,
  DEFAULT_ACCOUNT_CENTER,
  normalizeCenter,
  type AccountDependencies,
} from "./account-session";

afterEach(() => vi.useRealTimers());

function fixture() {
  vi.useFakeTimers();
  const host = {
    host_id: "remote",
    node_id: "other",
    server_id: "server-b",
    instance_id: "instance-b",
    name: "Office",
    platform: "linux",
    state: "online",
    f: ["credit", "download"],
  };
  const http = vi.fn(async (url: string | URL | Request, _init?: RequestInit) => {
    const path = String(url);
    if (path.endsWith("/auth/providers"))
      return Response.json({ authing_enabled: true, native_login_enabled: true });
    if (path.endsWith("/auth/client/exchange"))
      return Response.json({
        access_token: "private-user-token",
        expires_in: 3600,
        user: { display_name: "Alice", email: "alice@example.com" },
      });
    if (path.endsWith("/nodes/register"))
      return Response.json({
        node_id: "node",
        host_id: "self",
        node_session_id: "activation",
        control_required: true,
        hub_features: ["lease", "hosts", "state", "credit", "download", "bulk"],
        hub_ticket: "e".repeat(64),
        hub_path: "/v1/hub",
        hub_expires_in_seconds: 30,
        hub_ep: "access",
      });
    if (path.endsWith("/control-tickets"))
      return Response.json({ control_ticket: "short-lived-control-ticket" });
    if (path.endsWith("/relay-downloads"))
      return Response.json({
        relay_session_id: "visit",
        client_ticket: "one-use",
        server_id: "server-b",
        instance_id: "instance-b",
      });
    return new Response(null, { status: 204 });
  });
  const deps: AccountDependencies = {
    installationId: "stable-install",
    appVersion: "test",
    fetch: http as typeof fetch,
    browserLogin: {
      randomSecret: () => "b".repeat(64),
      challenge: async () => "test-challenge",
      open: async (_build, state) => ({
        redirectUri: "http://127.0.0.1:4567/auth/callback",
        url: `http://127.0.0.1:4567/auth/callback?state=${state}&code=${"c".repeat(64)}`,
      }),
    },
    runtime: () => ({
      status: "running",
      serverId: "server-a",
      instanceId: "instance-a",
      features: ["ait-rust-single-v1"],
    }),
    local: vi.fn(async () => ({ online: false, connecting: false })),
    save: vi.fn(async () => undefined),
    // Like account-ipc: the Hub link runs while signed in.
    notify: vi.fn((snapshot) => {
      if (snapshot.status === "logged_out") hub.link.stop();
      else hub.link.start();
    }),
    closeTransports: vi.fn(),
  };
  const manager = new AccountSessionManager(deps);
  const hub = centerLink(
    () => manager.openHubTicket(),
    () => [host],
  );
  manager.attachHub(hub.link);
  return {
    manager,
    deps,
    http,
    host,
    hub,
    failDiscovery: () => {
      hub.center.reachable = false;
      hub.socket().drop();
    },
  };
}

describe("account activation", () => {
  it("exchanges browser credentials and keeps them out of saved state and snapshots", async () => {
    const { manager, http, deps } = fixture();
    await manager.loginWithBrowser("");
    const [url, request] = http.mock.calls[1]!;
    expect(url).toBe("https://aitapp.cn/api/v1/auth/client/exchange");
    expect(request?.method).toBe("POST");
    expect(JSON.parse(String(request?.body))).toEqual({
      code: "c".repeat(64),
      code_verifier: "b".repeat(64),
    });
    expect(JSON.stringify(vi.mocked(deps.save).mock.calls)).not.toContain("b".repeat(64));
    expect(JSON.stringify(manager.snapshot())).not.toContain("private-user-token");
    await manager.logout();
  });

  it("uses the default HTTPS gateway for login, access registration and data", async () => {
    const { manager, http, deps, hub } = fixture();
    expect(manager.snapshot().center).toBe(DEFAULT_ACCOUNT_CENTER);
    await manager.loginWithBrowser("");
    await vi.advanceTimersByTimeAsync(1);
    expect(http.mock.calls[0]?.[0]).toBe("https://aitapp.cn/api/v1/auth/providers");
    expect(http.mock.calls.some(([url]) => String(url).endsWith("/relay-sessions"))).toBe(false);
    expect(deps.local).not.toHaveBeenCalled();
    expect(http.mock.calls.some(([url]) => String(url).endsWith("/control-tickets"))).toBe(false);
    expect(deps.save).toHaveBeenCalledWith(
      expect.objectContaining({ center: DEFAULT_ACCOUNT_CENTER }),
    );
    expect(hub.sockets.map((socket) => [socket.url, socket.options])).toEqual([
      ["wss://aitapp.cn/api/v1/hub", { headers: { Authorization: `Bearer ${"e".repeat(64)}` } }],
    ]);
    await manager.select("remote");
    expect(
      (await manager.openDownload("remote", "download-token", DEFAULT_ACCOUNT_CENTER)).url,
    ).toBe("wss://aitapp.cn/api/v1/relay/sessions/visit/client");
    expect(
      http.mock.calls.every(([url]) => String(url).startsWith(`${DEFAULT_ACCOUNT_CENTER}/v1/`)),
    ).toBe(true);
    await manager.logout();
  });

  it("restores a saved custom service without falling back to the default service", async () => {
    const { manager, http, deps } = fixture();
    await manager.restore({
      center: "https://private.example:9443/ait/api/",
      token: "saved-token",
      expiresAt: Date.now() + 3_600_000,
      name: "Alice",
      nodeSessionId: "previous-activation",
    });
    await vi.advanceTimersByTimeAsync(1);
    expect(manager.snapshot().center).toBe("https://private.example:9443/ait/api");
    expect(manager.snapshot().hosts).toHaveLength(1);
    expect(http.mock.calls.some(([url]) => String(url).includes("/auth/"))).toBe(false);
    expect(
      http.mock.calls.every(([url]) =>
        String(url).startsWith("https://private.example:9443/ait/api/v1/"),
      ),
    ).toBe(true);
    expect(deps.local).not.toHaveBeenCalled();
    expect(http.mock.calls.some(([url]) => String(url).endsWith("/control-tickets"))).toBe(false);
    await manager.logout();
  });

  it("auto-registers and discovers without opening a remote business session", async () => {
    const { manager, http, deps } = fixture();
    await manager.loginWithBrowser("http://127.0.0.1:3000");
    await vi.advanceTimersByTimeAsync(1);
    expect(manager.snapshot().hosts).toHaveLength(1);
    expect(http.mock.calls.some(([url]) => String(url).endsWith("/relay-sessions"))).toBe(false);
    expect(http.mock.calls.some(([url]) => String(url).includes("/credentials"))).toBe(false);
    expect(deps.local).not.toHaveBeenCalled();
    expect(http.mock.calls.some(([url]) => String(url).endsWith("/control-tickets"))).toBe(false);
    expect(JSON.stringify(manager.snapshot())).not.toContain("private-user-token");
    await manager.logout();
  });

  it("restores host access independently of selection and clears it on logout", async () => {
    const { manager, http, deps } = fixture();
    await manager.loginWithBrowser("https://center.example/api");
    await vi.advanceTimersByTimeAsync(1);
    const visit = await manager.openDownload(
      "remote",
      "download-token",
      "https://center.example/api",
    );
    expect(visit.url).toBe("wss://center.example/api/v1/relay/sessions/visit/client");
    await manager.logout();
    expect(manager.snapshot().selected).toBeNull();
    expect(deps.closeTransports).toHaveBeenCalled();
    expect(deps.save).toHaveBeenLastCalledWith(null);
    const count = http.mock.calls.length;
    await vi.advanceTimersByTimeAsync(120_000);
    expect(http.mock.calls).toHaveLength(count);
    await expect(
      manager.openDownload("remote", "download-token", "https://center.example/api"),
    ).rejects.toThrow();
  });

  it("keeps the last online list and selected host when discovery fails", async () => {
    const { manager, failDiscovery } = fixture();
    await manager.loginWithBrowser("https://center.example");
    await vi.advanceTimersByTimeAsync(1);
    await manager.select("remote");
    failDiscovery();
    await vi.advanceTimersByTimeAsync(12_000);
    expect(manager.snapshot().stale).toBe(true);
    expect(manager.snapshot().hosts).toHaveLength(1);
    expect(manager.snapshot().selected?.host_id).toBe("remote");
    await manager.logout();
  });

  it("replaces an expired registration id before retrying the same installation", async () => {
    const { manager, http } = fixture();
    const normal = http.getMockImplementation()!;
    const registrations: string[] = [];
    const installations: string[] = [];
    http.mockImplementation(async (url, ...args) => {
      if (String(url).endsWith("/nodes/register")) {
        const body = JSON.parse(String((args[0] as RequestInit | undefined)?.body));
        registrations.push(body.registration_id);
        installations.push(body.installation_id);
        if (registrations.length === 1)
          return Response.json(
            { error: { code: "registration_expired", message: "Expired" } },
            { status: 409 },
          );
      }
      return normal(url);
    });
    await manager.loginWithBrowser("https://center.example");
    await vi.advanceTimersByTimeAsync(6000);
    expect(registrations).toHaveLength(2);
    expect(registrations[0]).not.toBe(registrations[1]);
    expect(installations[0]).toBe(installations[1]);
    expect(manager.snapshot().hosts).toHaveLength(1);
    await manager.logout();
  });

  it("accepts HTTPS centers and loopback development only", () => {
    expect(normalizeCenter("  ")).toBe(DEFAULT_ACCOUNT_CENTER);
    expect(normalizeCenter("https://center.example/api/")).toBe("https://center.example/api");
    expect(() => normalizeCenter("http://example.com")).toThrow();
    expect(() => normalizeCenter("https://user:password@example.com")).toThrow();
    expect(() => normalizeCenter("https://example.com?token=secret")).toThrow();
  });
});

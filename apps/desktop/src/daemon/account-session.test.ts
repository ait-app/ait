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
    relay_modes: ["ait-rust-single-v1"],
  };
  let discoveryFails = false;
  const http = vi.fn(async (url: string | URL | Request, _init?: RequestInit) => {
    const path = String(url);
    if (path.endsWith("/auth/login"))
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
      });
    if (path.endsWith("/control-tickets"))
      return Response.json({ control_ticket: "short-lived-control-ticket" });
    if (path.includes("/hosts/online"))
      return discoveryFails
        ? Response.json({ error: { message: "Offline" } }, { status: 503 })
        : Response.json({ hosts: [host], count: 1 });
    if (path.endsWith("/relay-sessions") || path.endsWith("/relay-downloads"))
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
    runtime: () => ({
      status: "running",
      serverId: "server-a",
      instanceId: "instance-a",
      features: ["ait-rust-single-v1"],
    }),
    local: vi.fn(async () => ({ online: false, connecting: false })),
    save: vi.fn(async () => undefined),
    notify: vi.fn(),
    closeTransports: vi.fn(),
  };
  const manager = new AccountSessionManager(deps);
  return {
    manager,
    deps,
    http,
    host,
    failDiscovery: () => {
      discoveryFails = true;
    },
  };
}

describe("account activation", () => {
  it("logs in with normalized email and preserves the password exactly", async () => {
    const { manager, http, deps } = fixture();
    const password = "  a case-sensitive Password  ";
    await manager.login("", "  Alice@Example.COM  ", password);
    const [url, request] = http.mock.calls[0]!;
    expect(url).toBe("https://dash.ait-app.com:8443/api/v1/auth/login");
    expect(request?.method).toBe("POST");
    expect(JSON.parse(String(request?.body))).toEqual({
      email: "alice@example.com",
      password,
    });
    expect(JSON.stringify(vi.mocked(deps.save).mock.calls)).not.toContain(password);
    expect(JSON.stringify(manager.snapshot())).not.toContain(password);
    await manager.logout();
  });

  it("rejects blank emails before sending credentials or clearing the session", async () => {
    const { manager, http, deps } = fixture();
    await expect(manager.login("", "  ", "password")).rejects.toThrow("请输入邮箱和密码。");
    expect(http).not.toHaveBeenCalled();
    expect(deps.save).not.toHaveBeenCalled();
  });

  it("uses the default HTTPS gateway for login, registration, control and data", async () => {
    const { manager, http, deps } = fixture();
    expect(manager.snapshot().center).toBe(DEFAULT_ACCOUNT_CENTER);
    await manager.login("", "alice@example.com", "password");
    await vi.advanceTimersByTimeAsync(1);
    expect(http.mock.calls[0]?.[0]).toBe("https://dash.ait-app.com:8443/api/v1/auth/login");
    expect(http.mock.calls.some(([url]) => String(url).endsWith("/relay-sessions"))).toBe(false);
    expect(deps.local).toHaveBeenCalledWith("PUT", {
      center_url: "https://dash.ait-app.com:8443/api",
      control_ticket: "short-lived-control-ticket",
      node_session_id: "activation",
    });
    expect(deps.save).toHaveBeenCalledWith(
      expect.objectContaining({ center: DEFAULT_ACCOUNT_CENTER }),
    );
    await manager.select("remote");
    expect((await manager.openVisit("remote")).url).toBe(
      "wss://dash.ait-app.com:8443/api/v1/relay/sessions/visit/client",
    );
    expect((await manager.openDownload("remote", "download-token")).url).toBe(
      "wss://dash.ait-app.com:8443/api/v1/relay/sessions/visit/client",
    );
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
    expect(http.mock.calls.some(([url]) => String(url).endsWith("/auth/login"))).toBe(false);
    expect(
      http.mock.calls.every(([url]) =>
        String(url).startsWith("https://private.example:9443/ait/api/v1/"),
      ),
    ).toBe(true);
    expect(deps.local).toHaveBeenCalledWith("PUT", {
      center_url: "https://private.example:9443/ait/api",
      control_ticket: "short-lived-control-ticket",
      node_session_id: "activation",
    });
    await manager.logout();
  });

  it("auto-registers and discovers without opening a remote business session", async () => {
    const { manager, http, deps } = fixture();
    await manager.login("http://127.0.0.1:3000", "alice@example.com", "password");
    await vi.advanceTimersByTimeAsync(1);
    expect(manager.snapshot().hosts).toHaveLength(1);
    expect(http.mock.calls.some(([url]) => String(url).includes("exclude_node_id=node"))).toBe(
      true,
    );
    expect(http.mock.calls.some(([url]) => String(url).endsWith("/relay-sessions"))).toBe(false);
    expect(http.mock.calls.some(([url]) => String(url).includes("/credentials"))).toBe(false);
    expect(deps.local).toHaveBeenCalledWith("PUT", {
      center_url: "http://127.0.0.1:3000",
      control_ticket: "short-lived-control-ticket",
      node_session_id: "activation",
    });
    expect(JSON.stringify(manager.snapshot())).not.toContain("private-user-token");
    await manager.logout();
  });

  it("requires explicit selection and clears connection intent on logout", async () => {
    const { manager, http, deps } = fixture();
    await manager.login("https://center.example/api", "alice@example.com", "password");
    await vi.advanceTimersByTimeAsync(1);
    await expect(manager.openVisit("remote")).rejects.toThrow();
    await manager.select("remote");
    const visit = await manager.openVisit("remote");
    expect(visit.url).toBe("wss://center.example/api/v1/relay/sessions/visit/client");
    await manager.logout();
    expect(manager.snapshot().selected).toBeNull();
    expect(deps.closeTransports).toHaveBeenCalled();
    expect(deps.save).toHaveBeenLastCalledWith(null);
    const count = http.mock.calls.length;
    await vi.advanceTimersByTimeAsync(120_000);
    expect(http.mock.calls).toHaveLength(count);
    await expect(manager.openVisit("remote")).rejects.toThrow();
  });

  it("keeps the last online list and selected host when discovery fails", async () => {
    const { manager, failDiscovery } = fixture();
    await manager.login("https://center.example", "alice@example.com", "password");
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
    await manager.login("https://center.example", "alice@example.com", "password");
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

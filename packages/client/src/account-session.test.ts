import { afterEach, describe, expect, it, vi } from "vitest";
import { AccountSessionManager, type AccountDependencies } from "./account-session.js";

afterEach(() => vi.useRealTimers());

function fixture() {
  vi.useFakeTimers();
  const host = {
    host_id: "remote",
    node_id: "other",
    server_id: "server",
    instance_id: "instance",
    name: "Workstation",
    platform: "linux",
    relay_modes: ["ait-rust-single-v1"],
  };
  let leaseExpired = false;
  const http = vi.fn(async (url: string | URL | Request, _options?: RequestInit) => {
    const path = String(url);
    if (path.endsWith("/auth/login"))
      return Response.json({
        access_token: "private-jwt",
        expires_in: 3600,
        user: { email: "me@example.test" },
      });
    if (path.endsWith("/nodes/register"))
      return Response.json({
        node_id: "client",
        node_session_id: "lease",
        host_id: null,
        control_required: false,
      });
    if (path.includes("/hosts/online")) return Response.json({ hosts: [host] });
    if (path.endsWith("/renew") && leaseExpired) {
      leaseExpired = false;
      return Response.json({ error: { code: "node_session_expired" } }, { status: 401 });
    }
    if (path.endsWith("/relay-sessions"))
      return Response.json({
        relay_session_id: "visit",
        client_ticket: "once",
        server_id: host.server_id,
        instance_id: host.instance_id,
      });
    return new Response(null, { status: 204 });
  });
  let sequence = 0;
  const deps: AccountDependencies = {
    installationId: "stable-mobile-installation",
    deviceName: "Phone",
    platform: "android",
    appVersion: "test",
    randomUUID: () => `uuid-${++sequence}`,
    runtime: () => ({ status: "stopped", serverId: "" }),
    local: vi.fn(async () => undefined),
    fetch: http,
    save: vi.fn(async () => undefined),
    notify: vi.fn(),
    closeTransports: vi.fn(),
  };
  const manager = new AccountSessionManager(deps);
  const login = async () => {
    await manager.login("", "ME@example.test", " secret ");
    await vi.advanceTimersByTimeAsync(1);
  };
  return {
    manager,
    http,
    deps,
    host,
    login,
    expireLease: () => {
      leaseExpired = true;
    },
  };
}

describe("client-only account lifecycle", () => {
  it("registers Android without publishing a host, discovers and renews independently of visits", async () => {
    const { manager, http, deps, login, host } = fixture();
    await login();
    const registration = http.mock.calls.find(([url]) => String(url).endsWith("/nodes/register"))!;
    expect(JSON.parse(String(registration[1]?.body))).toMatchObject({
      runtime: null,
      platform: "android",
      display_name: "Phone",
      installation_id: "stable-mobile-installation",
    });
    expect(manager.snapshot()).toMatchObject({
      status: "online",
      hostOnline: false,
      hosts: [host],
      selected: null,
    });
    expect(http.mock.calls.some(([url]) => String(url).includes("relay-sessions"))).toBe(false);
    expect(deps.local).not.toHaveBeenCalledWith("PUT", expect.anything());
    await vi.advanceTimersByTimeAsync(20_000);
    expect(http.mock.calls.some(([url]) => String(url).endsWith("/renew"))).toBe(true);
    expect(JSON.stringify(manager.snapshot())).not.toContain("private-jwt");
    expect(JSON.stringify(vi.mocked(deps.save).mock.calls)).not.toContain(" secret ");
    await manager.logout();
  });

  it("only grants visits to explicitly selected hosts and revokes them on logout", async () => {
    const { manager, deps, login } = fixture();
    await login();
    await expect(manager.openVisit("remote")).rejects.toThrow("not selected");
    await manager.select("remote");
    expect(await manager.openVisit("remote")).toMatchObject({
      url: "wss://dash.ait-app.com:8443/api/v1/relay/sessions/visit/client",
    });
    await manager.logout();
    expect(deps.closeTransports).toHaveBeenCalled();
    expect(deps.save).toHaveBeenLastCalledWith(null);
    await expect(manager.openVisit("remote")).rejects.toThrow();
  });

  it("stops network activity in background and renews/re-registers an expired lease on resume", async () => {
    const { manager, http, deps, login, expireLease } = fixture();
    await login();
    await manager.select("remote");
    manager.suspend();
    const count = http.mock.calls.length;
    await vi.advanceTimersByTimeAsync(120_000);
    expect(http).toHaveBeenCalledTimes(count);
    await expect(manager.openVisit("remote")).rejects.toThrow();
    expect(deps.closeTransports).toHaveBeenCalled();
    expireLease();
    manager.resume();
    await vi.advanceTimersByTimeAsync(8000);
    expect(http.mock.calls.filter(([url]) => String(url).endsWith("/nodes/register"))).toHaveLength(
      2,
    );
    expect(manager.snapshot()).toMatchObject({ status: "online", selected: { host_id: "remote" } });
    await manager.logout();
  });

  it("restores a valid saved account and releases the previous activation before registering", async () => {
    const { manager, http } = fixture();
    await manager.restore({
      center: "https://example.test/api",
      token: "saved-jwt",
      expiresAt: Date.now() + 60_000,
      name: "Me",
      nodeSessionId: "old",
    });
    expect(http.mock.calls[0]).toEqual([
      "https://example.test/api/v1/node-sessions/old",
      expect.objectContaining({ method: "DELETE" }),
    ]);
    await vi.advanceTimersByTimeAsync(1);
    expect(manager.snapshot().status).toBe("online");
    await manager.logout();
  });
});

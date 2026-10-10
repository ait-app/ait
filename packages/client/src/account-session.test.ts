import { afterEach, describe, expect, it, vi } from "vitest";
import {
  AccountSessionManager,
  type AccountDependencies,
  type SavedAccount,
} from "./account-session.js";
import { createHash } from "node:crypto";

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
    if (path.endsWith("/nodes/register")) {
      const runtime = JSON.parse(String(_options?.body)).runtime;
      return Response.json({
        node_id: "client",
        node_session_id: runtime ? `lease-${runtime.server_id}` : "lease",
        host_id: runtime ? `host-${runtime.server_id}` : null,
        control_required: Boolean(runtime),
      });
    }
    if (path.endsWith("/control-tickets")) return Response.json({ control_ticket: "a".repeat(64) });
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

function hostIdentityFixture() {
  const context = fixture();
  const original = context.http.getMockImplementation()!;
  const installations = new Map<string, string>();
  context.http.mockImplementation((url, options) => {
    if (String(url).endsWith("/nodes/register")) {
      const body = JSON.parse(String(options?.body));
      if (body.runtime) {
        const serverId = body.runtime.server_id;
        const installation = installations.get(serverId);
        // mirrors nodes.host_id UNIQUE: closing a session never removes its node binding
        if (installation && installation !== body.installation_id)
          return Promise.resolve(
            Response.json(
              { error: { code: "conflict", message: "Resource already exists" } },
              { status: 409 },
            ),
          );
        installations.set(serverId, body.installation_id);
      }
    }
    return original(url, options);
  });
  return context;
}

function browserFixture() {
  const context = fixture();
  const original = context.http.getMockImplementation()!;
  let count = 0;
  const open = vi.fn(async (build: (uri: string) => string) => {
    const redirectUri = "ait://auth/callback";
    const url = new URL(build(redirectUri));
    expect(url.searchParams.get("redirect_uri")).toBe(redirectUri);
    expect(url.searchParams.get("code_challenge")).toBe(
      createHash("sha256").update("b".repeat(64)).digest("base64url"),
    );
    return {
      redirectUri,
      url: `${redirectUri}?state=${url.searchParams.get("state")}&code=${"c".repeat(64)}`,
    };
  });
  context.deps.browserLogin = {
    randomSecret: () => (++count === 1 ? "a" : "b").repeat(64),
    challenge: async (value) => createHash("sha256").update(value).digest("base64url"),
    open,
  };
  context.http.mockImplementation((url, options) => {
    if (String(url).endsWith("/auth/providers"))
      return Promise.resolve(Response.json({ authing_enabled: true, native_login_enabled: true }));
    if (String(url).endsWith("/auth/client/exchange"))
      return Promise.resolve(
        Response.json({
          access_token: "browser-jwt",
          expires_in: 3600,
          user: { email: "browser@example.test", expires_at: "2030-01-01T00:00:00Z" },
        }),
      );
    return original(url, options);
  });
  return { ...context, open };
}

describe("hosted account login", () => {
  it.each(["android", "ios", "darwin"])(
    "accepts a phone-only session on %s and discovers hosts",
    async (platform) => {
      const { manager, http, deps } = browserFixture();
      deps.platform = platform;
      const original = http.getMockImplementation()!;
      http.mockImplementation((url, options) =>
        String(url).endsWith("/auth/client/exchange")
          ? Promise.resolve(
              Response.json({
                access_token: "phone-jwt",
                expires_in: 3600,
                user: { email: null, phone_number: "+8613800138000", display_name: null },
              }),
            )
          : original(url, options),
      );
      await manager.loginWithBrowser("");
      await vi.advanceTimersByTimeAsync(1);
      expect(manager.snapshot()).toMatchObject({ name: "+8613800138000", status: "online" });
      expect(manager.snapshot().hosts).toHaveLength(1);
      expect(deps.save).toHaveBeenCalledWith(
        expect.objectContaining({ token: "phone-jwt", name: "+8613800138000" }),
      );
      await manager.logout();
    },
  );

  it.each([
    {},
    { email: null },
    { phone_number: {} },
    { phone_number: " " },
    { display_name: "Name only" },
  ])("rejects a session without a usable contact: %j", async (user) => {
    const { manager, http, deps } = browserFixture();
    const original = http.getMockImplementation()!;
    http.mockImplementation((url, options) =>
      String(url).endsWith("/auth/client/exchange")
        ? Promise.resolve(Response.json({ access_token: "jwt", expires_in: 3600, user }))
        : original(url, options),
    );
    await expect(manager.loginWithBrowser("")).rejects.toThrow("invalid login session");
    expect(vi.mocked(deps.save).mock.calls.every(([value]) => value === null)).toBe(true);
  });

  it("keeps the renewal explanation after an expired account is signed out", async () => {
    const { manager, http, deps } = browserFixture();
    const original = http.getMockImplementation()!;
    http.mockImplementation((url, options) =>
      String(url).endsWith("/nodes/register")
        ? Promise.resolve(
            Response.json(
              {
                error: {
                  code: "account_expired",
                  message: "Account expired; contact your administrator to renew access.",
                },
              },
              { status: 401 },
            ),
          )
        : original(url, options),
    );
    await manager.loginWithBrowser("");
    await vi.advanceTimersByTimeAsync(1);
    expect(manager.snapshot()).toMatchObject({
      status: "logged_out",
      error: expect.stringContaining("renew"),
    });
    expect(deps.save).toHaveBeenLastCalledWith(null);
  });
  it("works with the native AbortSignal without throwIfAborted", async () => {
    const { manager } = browserFixture();
    const PlatformAbortController = class extends AbortController {
      constructor() {
        super();
        Object.defineProperty(this.signal, "throwIfAborted", { value: undefined });
      }
    };
    vi.stubGlobal("AbortController", PlatformAbortController);
    try {
      await manager.loginWithBrowser("");
      expect(manager.snapshot().name).toBe("browser@example.test");
      await manager.logout();
    } finally {
      vi.unstubAllGlobals();
    }
  });
  it("exchanges native PKCE, persists only the AIT session and discovers hosts", async () => {
    const { manager, deps, http } = browserFixture();
    expect((await manager.loginWithBrowser("")).loginPending).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    const exchange = http.mock.calls.find(([url]) => String(url).endsWith("/client/exchange"))!;
    expect(JSON.parse(String(exchange[1]?.body))).toEqual({
      code: "c".repeat(64),
      code_verifier: "b".repeat(64),
    });
    expect(manager.snapshot()).toMatchObject({
      status: "online",
      name: "browser@example.test",
      accountExpiresAt: "2030-01-01T00:00:00Z",
      loginPending: false,
    });
    expect(JSON.stringify(manager.snapshot())).not.toContain("browser-jwt");
    expect(JSON.stringify(vi.mocked(deps.save).mock.calls)).not.toContain("b".repeat(64));
    await manager.logout();
  });
  it("keeps browser login alive while Android backgrounds for the system browser", async () => {
    const { manager, open } = browserFixture();
    const original = open.getMockImplementation()!;
    open.mockImplementation(async (build) => {
      manager.suspend();
      const response = await original(build);
      manager.resume();
      return response;
    });
    await manager.loginWithBrowser("");
    expect(manager.snapshot().name).toBe("browser@example.test");
    await manager.logout();
  });
  it("rejects a foreign callback without exchanging it", async () => {
    const { manager, open, http } = browserFixture();
    open.mockResolvedValue({
      redirectUri: "ait://auth/callback",
      url: `ait://auth/callback?state=wrong&code=${"c".repeat(64)}`,
    });
    await expect(manager.loginWithBrowser("")).rejects.toThrow("callback is invalid");
    expect(http.mock.calls.some(([url]) => String(url).endsWith("/client/exchange"))).toBe(false);
    expect(manager.snapshot()).toMatchObject({ status: "logged_out", loginPending: false });
  });
  it.each(["cancel", "timeout"])(
    "ends browser login on %s before accepting credentials",
    async (reason) => {
      const { manager, deps } = browserFixture();
      deps.browserLogin!.open = async (_build, _state, signal) =>
        new Promise((_resolve, reject) => {
          signal.addEventListener("abort", () => reject(new Error("cancelled")), { once: true });
        });
      const pending = manager.loginWithBrowser("");
      const failure = expect(pending).rejects.toThrow("cancelled");
      await vi.advanceTimersByTimeAsync(0);
      expect(manager.snapshot().loginPending).toBe(true);
      if (reason === "cancel") manager.cancelLogin();
      else await vi.advanceTimersByTimeAsync(10 * 60_000);
      await failure;
      expect(manager.snapshot()).toMatchObject({ status: "logged_out", loginPending: false });
    },
  );
});

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

describe("account persistence across restarts", () => {
  it("keeps a fourteen-day login across shutdown and an overnight restart without extending it", async () => {
    const { manager, deps, http, login } = fixture();
    let persisted: SavedAccount | null = null;
    vi.mocked(deps.save).mockImplementation(async (value) => {
      persisted = value ? { ...value } : null;
    });
    http.mockResolvedValueOnce(
      Response.json({
        access_token: "private-jwt",
        expires_in: 14 * 86400,
        user: { email: "me@example.test" },
      }),
    );
    await login();
    const beforeShutdown = persisted;
    expect(beforeShutdown).toMatchObject({ token: "private-jwt", nodeSessionId: "lease" });
    vi.mocked(deps.save).mockClear();

    await manager.shutdown();
    await manager.shutdown();
    expect(deps.save).not.toHaveBeenCalled();
    expect(persisted).toEqual(beforeShutdown);
    expect(http.mock.calls).toContainEqual([
      expect.stringContaining("/node-sessions/lease"),
      expect.objectContaining({ method: "DELETE" }),
    ]);
    const requestsAfterShutdown = http.mock.calls.length;
    await vi.advanceTimersByTimeAsync(86400_000);
    expect(http).toHaveBeenCalledTimes(requestsAfterShutdown);

    const restarted = new AccountSessionManager(deps);
    await restarted.restore(persisted!);
    await vi.advanceTimersByTimeAsync(1);
    expect(restarted.snapshot()).toMatchObject({ status: "online", name: "me@example.test" });
    expect(persisted).toEqual(beforeShutdown);
    expect(http.mock.calls.filter(([url]) => String(url).endsWith("/auth/login"))).toHaveLength(1);
    await restarted.logout();
    expect(persisted).toBeNull();
  });

  it("can close without touching credentials when storage becomes unavailable", async () => {
    const { manager, deps, login } = fixture();
    await login();
    vi.mocked(deps.save).mockClear().mockRejectedValue(new Error("Secure storage unavailable"));
    await expect(manager.shutdown()).resolves.toBeUndefined();
    expect(deps.save).not.toHaveBeenCalled();
    expect(deps.closeTransports).toHaveBeenCalled();
  });

  it("does not restore expired credentials or prolong their validity on shutdown", async () => {
    const { manager, deps, http } = fixture();
    await manager.restore({
      center: "https://example.test/api",
      token: "expired-jwt",
      expiresAt: Date.now(),
      name: "Me",
    });
    await vi.advanceTimersByTimeAsync(1);
    expect(manager.snapshot().status).toBe("logged_out");
    await manager.shutdown();
    expect(http).not.toHaveBeenCalled();
    expect(deps.save).not.toHaveBeenCalled();
  });
});

describe("explicit daemon publication", () => {
  const first = {
    serverId: "first",
    instanceId: "instance-1",
    name: "First host",
    platform: "linux",
  };
  const second = {
    serverId: "second",
    instanceId: "instance-2",
    name: "Second host",
    platform: "darwin",
  };

  it("reuses the host's node after stopping synchronization and after a daemon restart", async () => {
    const { manager, http, login } = hostIdentityFixture();
    await login();
    await manager.publishHost(first);
    await manager.unpublishHost(first.serverId);
    await expect(manager.publishHost(first)).resolves.toMatchObject({
      node_session_id: "lease-first",
    });
    await expect(manager.publishHost({ ...first, instanceId: "restarted" })).resolves.toMatchObject(
      { node_session_id: "lease-first" },
    );
    const registrations = http.mock.calls
      .filter(([url]) => String(url).endsWith("/nodes/register"))
      .map(([, options]) => JSON.parse(String(options?.body)))
      .filter((body) => body.runtime);
    expect(registrations).toHaveLength(3);
    expect(registrations.map((body) => body.installation_id)).toEqual([
      first.serverId,
      first.serverId,
      first.serverId,
    ]);
    expect(new Set(registrations.map((body) => body.registration_id)).size).toBe(3);
    await manager.unpublishHost(first.serverId);
    await manager.logout();
  });

  it("reuses the same host identity when another client enables synchronization after lease expiry", async () => {
    const { manager, deps, login } = hostIdentityFixture();
    await login();
    await manager.publishHost(first);
    await manager.shutdown();
    await vi.advanceTimersByTimeAsync(61_000);
    const next = new AccountSessionManager({ ...deps, installationId: "another-client" });
    await next.login("", "ME@example.test", " secret ");
    await vi.advanceTimersByTimeAsync(1);
    await expect(next.publishHost(first)).resolves.toMatchObject({
      node_session_id: "lease-first",
    });
    await next.unpublishHost(first.serverId);
    await next.logout();
  });

  it("replaces a closed registration ID while preserving the daemon installation identity", async () => {
    const { manager, http, login } = fixture();
    await login();
    http.mockResolvedValueOnce(
      Response.json(
        { error: { code: "registration_expired", message: "Registration is closed" } },
        { status: 409 },
      ),
    );
    await expect(manager.publishHost(first)).rejects.toThrow("Registration is closed");
    await expect(manager.publishHost(first)).resolves.toMatchObject({
      node_session_id: "lease-first",
    });
    const registrations = http.mock.calls
      .filter(([url]) => String(url).endsWith("/nodes/register"))
      .map(([, options]) => JSON.parse(String(options?.body)))
      .filter((body) => body.runtime);
    expect(registrations).toHaveLength(2);
    expect(registrations[0].registration_id).not.toBe(registrations[1].registration_id);
    expect(registrations.map((body) => body.installation_id)).toEqual([
      first.serverId,
      first.serverId,
    ]);
    await manager.unpublishHost(first.serverId);
    await manager.logout();
  });

  it("keeps desktop login independent from daemon publication and binds grants to each host", async () => {
    const { manager, deps, http, login } = fixture();
    deps.publishRuntime = false;
    deps.runtime = () => ({
      status: "running",
      serverId: "local",
      instanceId: "local-instance",
      features: ["ait-rust-single-v1"],
    });
    await login();
    expect(JSON.parse(String(http.mock.calls[1][1]?.body)).runtime).toBeNull();
    const grant = await manager.publishHost(first);
    expect(grant).toEqual({
      center_url: "https://dash.ait-app.com:8443/api",
      control_ticket: "a".repeat(64),
      node_session_id: "lease-first",
    });
    await manager.publishHost(second);
    await manager.publishHost(first, false);
    const registrations = http.mock.calls.filter(([url]) =>
      String(url).endsWith("/nodes/register"),
    );
    expect(registrations).toHaveLength(3);
    expect(JSON.parse(String(registrations[1][1]?.body))).toMatchObject({
      display_name: "First host",
      runtime: { server_id: "first", instance_id: "instance-1" },
    });
    expect(JSON.parse(String(registrations[2][1]?.body))).toMatchObject({
      runtime: { server_id: "second", instance_id: "instance-2" },
    });
    expect(JSON.stringify(grant)).not.toContain("private-jwt");
    expect(deps.local).not.toHaveBeenCalledWith("PUT", expect.anything());
    await vi.advanceTimersByTimeAsync(22_000);
    for (const id of ["first", "second"])
      expect(
        http.mock.calls.some(([url]) => String(url).endsWith(`/node-sessions/lease-${id}/renew`)),
      ).toBe(true);
    await manager.unpublishHost("first");
    const before = http.mock.calls.length;
    await vi.advanceTimersByTimeAsync(22_000);
    expect(
      http.mock.calls.slice(before).some(([url]) => String(url).endsWith("/lease-first/renew")),
    ).toBe(false);
    await manager.logout();
    expect(http.mock.calls).not.toContainEqual([
      expect.stringContaining("/node-sessions/lease-second"),
      expect.objectContaining({ method: "DELETE" }),
    ]);
    expect(manager.snapshot()).toMatchObject({
      status: "logged_out",
      synchronizedHosts: ["second"],
    });
    const afterLogout = http.mock.calls.length;
    await vi.advanceTimersByTimeAsync(22_000);
    expect(http.mock.calls.slice(afterLogout)).toContainEqual([
      expect.stringContaining("/node-sessions/lease-second/renew"),
      expect.objectContaining({ method: "POST" }),
    ]);
    expect(
      http.mock.calls.slice(afterLogout).some(([url]) => String(url).includes("/hosts/online")),
    ).toBe(false);
    await manager.unpublishHost("second");
  });

  it("replaces a restarted daemon's lease and requires a signed-in account", async () => {
    const { manager, http, login } = fixture();
    await expect(manager.publishHost(first)).rejects.toThrow("Sign in first");
    await login();
    await expect(manager.publishHost({ ...first, instanceId: "" })).rejects.toThrow(
      "Invalid host identity",
    );
    await manager.publishHost(first);
    await manager.publishHost({ ...first, instanceId: "restarted" });
    expect(http.mock.calls).toContainEqual([
      expect.stringContaining("/node-sessions/lease-first"),
      expect.objectContaining({ method: "DELETE" }),
    ]);
    expect(http.mock.calls.filter(([url]) => String(url).endsWith("/nodes/register"))).toHaveLength(
      3,
    );
    await manager.logout();
  });

  it("releases only the binding daemon on logout and lets remote daemons reconnect and stop while signed out", async () => {
    const { manager, deps, http, login } = fixture();
    deps.publishRuntime = false;
    deps.runtime = () => ({ status: "running", serverId: "first", instanceId: first.instanceId });
    await login();
    await manager.publishHost(first);
    await manager.publishHost(second);
    await manager.logout();
    expect(manager.snapshot()).toMatchObject({
      status: "logged_out",
      synchronizedHosts: ["second"],
    });
    const deleted = http.mock.calls
      .filter(([, options]) => options?.method === "DELETE")
      .map(([url]) => String(url));
    expect(deleted).toContainEqual(expect.stringContaining("/node-sessions/lease"));
    expect(deleted).toContainEqual(expect.stringContaining("/node-sessions/lease-first"));
    expect(deleted).not.toContainEqual(expect.stringContaining("/node-sessions/lease-second"));
    await expect(manager.publishHost(first)).rejects.toThrow("Sign in first");
    expect(await manager.publishHost(second)).toMatchObject({ node_session_id: "lease-second" });
    const before = http.mock.calls.length;
    await vi.advanceTimersByTimeAsync(22_000);
    const renewals = http.mock.calls
      .slice(before)
      .filter(([url]) => String(url).endsWith("/renew"));
    expect(renewals).toHaveLength(1);
    expect(String(renewals[0][0])).toContain("lease-second/renew");
    await manager.unpublishHost("second");
    const stopped = http.mock.calls.length;
    await vi.advanceTimersByTimeAsync(22_000);
    expect(http).toHaveBeenCalledTimes(stopped);
    expect(JSON.stringify(manager.snapshot())).not.toContain("private-jwt");
  });

  it("keeps a daemon's original account authority when the client signs into another service", async () => {
    const { manager, http, login } = fixture();
    await login();
    await manager.publishHost(second);
    await manager.login("https://other.test/api", "other@example.test", "other secret");
    await vi.advanceTimersByTimeAsync(22_000);
    expect(http.mock.calls).toContainEqual([
      "https://dash.ait-app.com:8443/api/v1/node-sessions/lease-second/renew",
      expect.objectContaining({ method: "POST" }),
    ]);
    expect(await manager.publishHost(second)).toMatchObject({
      center_url: "https://dash.ait-app.com:8443/api",
    });
    await manager.unpublishHost("second");
    await manager.logout();
  });

  it("does not revoke remote leases when the app shuts down", async () => {
    const { manager, http, login } = fixture();
    await login();
    await manager.publishHost(second);
    await manager.shutdown();
    expect(http.mock.calls).not.toContainEqual([
      expect.stringContaining("/node-sessions/lease-second"),
      expect.objectContaining({ method: "DELETE" }),
    ]);
    const before = http.mock.calls.length;
    await vi.advanceTimersByTimeAsync(22_000);
    expect(http).toHaveBeenCalledTimes(before);
  });

  it("does not abort a remote registration in flight when the client signs out", async () => {
    const { manager, http, login } = fixture();
    await login();
    let complete: (response: Response) => void = () => {};
    http.mockImplementationOnce(
      () =>
        new Promise<Response>((resolve) => {
          complete = resolve;
        }),
    );
    const publishing = manager.publishHost(second);
    const signal = http.mock.calls.at(-1)?.[1]?.signal;
    await manager.logout();
    expect(signal?.aborted).toBe(false);
    complete(
      Response.json({
        node_id: "second",
        node_session_id: "lease-second",
        host_id: "second",
        control_required: true,
      }),
    );
    expect(await publishing).toMatchObject({ node_session_id: "lease-second" });
    await manager.unpublishHost("second");
  });

  it("isolates a daemon authorization failure from the client and other daemon renewals", async () => {
    const { manager, http, login } = fixture();
    const original = http.getMockImplementation()!;
    http.mockImplementation((url, options) =>
      String(url).endsWith("/lease-first/renew")
        ? Promise.resolve(Response.json({ error: { code: "unauthorized" } }, { status: 401 }))
        : original(url, options),
    );
    await login();
    await manager.publishHost(first);
    await manager.publishHost(second);
    await vi.advanceTimersByTimeAsync(44_000);
    expect(manager.snapshot()).toMatchObject({
      status: "online",
      synchronizedHosts: ["first", "second"],
    });
    expect(
      http.mock.calls.filter(([url]) => String(url).endsWith("/lease-second/renew")),
    ).toHaveLength(2);
    expect(http.mock.calls.some(([, options]) => options?.method === "DELETE")).toBe(false);
    await manager.unpublishHost("first");
    await manager.unpublishHost("second");
    await manager.logout();
  });

  it("stops renewing the binding daemon even if its logout revocation cannot reach the service", async () => {
    const { manager, deps, http, login } = fixture();
    const original = http.getMockImplementation()!;
    http.mockImplementation((url, options) =>
      String(url).endsWith("/lease-first") && options?.method === "DELETE"
        ? Promise.resolve(Response.json({ error: { code: "unavailable" } }, { status: 503 }))
        : original(url, options),
    );
    deps.publishRuntime = false;
    deps.runtime = () => ({ status: "running", serverId: first.serverId });
    await login();
    await manager.publishHost(first);
    await manager.publishHost(second);
    await manager.logout();
    expect(manager.snapshot().synchronizedHosts).toEqual(["second"]);
    const before = http.mock.calls.length;
    await vi.advanceTimersByTimeAsync(22_000);
    expect(
      http.mock.calls.slice(before).some(([url]) => String(url).endsWith("/lease-first/renew")),
    ).toBe(false);
    expect(
      http.mock.calls.slice(before).some(([url]) => String(url).endsWith("/lease-second/renew")),
    ).toBe(true);
    await manager.unpublishHost("second");
  });
});

import { channelFixture, welcome } from "../test/account-channel.js";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  AccountSessionManager,
  normalizeCenter,
  type AccountDependencies,
  type SavedAccount,
} from "./account-session.js";
import { createHash } from "node:crypto";

afterEach(() => vi.useRealTimers());

describe("account center domain migration", () => {
  it.each([
    "",
    "https://dash.ait-app.com:8443/api",
    " https://DASH.ait-app.com:8443/api/ ",
    "https://aitapp.cn:8443/api",
    " https://AITAPP.cn:8443/api/ ",
    "https://aitapp.cn/api",
    "https://aitapp.cn:443/api",
  ])("uses the new official endpoint for %j", (center) => {
    expect(normalizeCenter(center)).toBe("https://aitapp.cn/api");
  });

  it.each([
    "https://custom.test:9443/ait/api",
    "https://dash.ait-app.com/api",
    "https://dash.ait-app.com:8443/custom/api",
    "https://dash.ait-app.com.example.test:8443/api",
    "http://127.0.0.1:3000/api",
    "https://aitapp.cn:9443/api",
    "https://aitapp.cn:8443/custom/api",
    "https://aitapp.cn.example.test:8443/api",
  ])("preserves a custom service endpoint: %s", (center) => {
    expect(normalizeCenter(center)).toBe(center);
  });

  it.each([
    "http://aitapp.cn:8443/api",
    "https://user:password@aitapp.cn:8443/api",
    "https://aitapp.cn:8443/api?token=secret",
    "https://aitapp.cn:8443/api#fragment",
    "http://dash.ait-app.com:8443/api",
    "https://user:password@dash.ait-app.com:8443/api",
    "https://dash.ait-app.com:8443/api?token=secret",
    "https://dash.ait-app.com:8443/api#fragment",
  ])("still rejects invalid service URLs before migration: %s", (center) => {
    expect(() => normalizeCenter(center)).toThrow();
  });

  it.each(["https://dash.ait-app.com:8443/api", "https://aitapp.cn:8443/api"])(
    "restores %s and uses the new origin for API, saved state and relay",
    async (center) => {
      const { manager, http, deps } = fixture();
      const saved = {
        center,
        token: "saved-jwt",
        expiresAt: Date.now() + 60_000,
        name: "Me",
        nodeSessionId: "old",
      };
      await manager.restore(saved);
      expect(http.mock.calls[0]).toEqual([
        "https://aitapp.cn/api/v1/node-sessions/old",
        expect.objectContaining({ method: "DELETE" }),
      ]);
      await vi.advanceTimersByTimeAsync(1);
      expect(manager.snapshot()).toMatchObject({
        status: "online",
        center: "https://aitapp.cn/api",
      });
      expect(deps.save).toHaveBeenCalledWith(
        expect.objectContaining({
          center: "https://aitapp.cn/api",
          token: saved.token,
          expiresAt: saved.expiresAt,
        }),
      );
      const visit = await manager.openVisit("remote", saved.center);
      expect(visit.url).toBe("wss://aitapp.cn/api/v1/relay/sessions/visit/client");
      expect(
        http.mock.calls.every(([url]) => String(url).startsWith("https://aitapp.cn/api/")),
      ).toBe(true);
      await manager.logout();
    },
  );
});

function fixture(autoChannel = true) {
  vi.useFakeTimers();
  const host = {
    online: true,
    status: "offline",
    host_id: "remote",
    node_id: "other",
    server_id: "server",
    instance_id: "instance",
    name: "Workstation",
    platform: "linux",
    relay_modes: ["ait-rust-single-v1"],
  };
  const channel = channelFixture(() => [host], autoChannel);
  let leaseExpired = false;
  const http = vi.fn(async (url: string | URL | Request, _options?: RequestInit) => {
    const path = String(url);
    if (path.endsWith("/auth/providers"))
      return Response.json({ authing_enabled: true, native_login_enabled: true });
    if (path.endsWith("/auth/client/exchange"))
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
    if (path.endsWith("/channel-tickets"))
      return Response.json({
        channel_ticket: "one-use-channel",
        channel_path: "/v1/relay/channel",
        protocol: "ait-channel-v1",
      });
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
    channelTransport: channel.create,
    browserLogin: {
      randomSecret: () => "b".repeat(64),
      challenge: async (value) => createHash("sha256").update(value).digest("base64url"),
      open: async (_build, state) => ({
        redirectUri: "ait://auth/callback",
        url: `ait://auth/callback?state=${state}&code=${"c".repeat(64)}`,
      }),
    },
    save: vi.fn(async () => undefined),
    notify: vi.fn(),
    closeTransports: vi.fn(),
  };
  const manager = new AccountSessionManager(deps);
  const login = async () => {
    await manager.loginWithBrowser("");
    await vi.advanceTimersByTimeAsync(1);
  };
  return {
    manager,
    http,
    deps,
    host,
    channel,
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
  it.each([
    { status: 404, options: {} },
    { status: 404, options: "<html>Not Found</html>" },
    { status: 200, options: {} },
    { status: 200, options: { authing_enabled: true, native_login_enabled: false } },
    { status: 200, options: { authing_enabled: false, native_login_enabled: true } },
  ])(
    "rejects unsupported services without a password fallback: %j",
    async ({ status, options }) => {
      const { manager, http, deps, open } = browserFixture();
      await manager.restore({
        center: "https://saved.test/api",
        token: "saved-token",
        expiresAt: Date.now() + 3600_000,
        name: "Saved account",
      });
      http.mockResolvedValueOnce(
        typeof options === "string"
          ? new Response(options, { status, headers: { "Content-Type": "text/html" } })
          : Response.json(options, { status }),
      );
      await expect(manager.loginWithBrowser("https://other.test/api")).rejects.toThrow(
        "Update the service or choose another service URL",
      );
      expect(http).toHaveBeenCalledExactlyOnceWith(
        "https://other.test/api/v1/auth/providers",
        expect.objectContaining({ method: "GET" }),
      );
      expect(open).not.toHaveBeenCalled();
      expect(deps.save).not.toHaveBeenCalled();
      expect(manager.snapshot()).toMatchObject({
        center: "https://saved.test/api",
        name: "Saved account",
      });
      await manager.logout();
    },
  );

  it("reports discovery failure and allows retrying browser login", async () => {
    const { manager, http, deps, open } = browserFixture();
    http.mockRejectedValueOnce(new TypeError("Network request failed"));
    await expect(manager.loginWithBrowser("")).rejects.toThrow("Network request failed");
    expect(open).not.toHaveBeenCalled();
    expect(deps.save).not.toHaveBeenCalled();
    await manager.loginWithBrowser("");
    expect(manager.snapshot().name).toBe("browser@example.test");
    expect(http.mock.calls.some(([url]) => String(url).endsWith("/auth/login"))).toBe(false);
    await manager.logout();
  });

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
  it("keeps existing host visits available when another host is selected or deselected", async () => {
    const { manager, deps, login, channel, host } = fixture();
    await login();
    channel.sockets[0]!.receive({
      type: "hosts.snapshot",
      hosts: [host, { ...host, host_id: "another" }],
      observed_at: "old",
    });
    vi.mocked(deps.closeTransports).mockClear();
    const center = manager.snapshot().center;
    await manager.select("remote");
    await manager.openVisit("remote", center);
    await manager.select("another");
    await manager.openVisit("remote", center);
    await manager.openVisit("another", center);
    await manager.select(null);
    await manager.openVisit("remote", center);
    expect(deps.closeTransports).not.toHaveBeenCalled();
    await manager.logout();
    expect(deps.closeTransports).toHaveBeenCalledOnce();
  });

  it("rejects saved hosts from another service before sending account credentials", async () => {
    const { manager, login, http } = fixture();
    await login();
    const count = http.mock.calls.length;
    await expect(manager.openVisit("remote", "https://other.test/api")).rejects.toThrow(
      "Sign in to the online service used by this host.",
    );
    await expect(manager.openDownload("remote", "token", "https://other.test/api")).rejects.toThrow(
      "Sign in to the online service used by this host.",
    );
    expect(http).toHaveBeenCalledTimes(count);
    await manager.logout();
  });

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

  it("reconnects saved hosts without a new selection and revokes visits on logout", async () => {
    const { manager, deps, login } = fixture();
    await login();
    expect(await manager.openVisit("remote", manager.snapshot().center)).toMatchObject({
      url: "wss://aitapp.cn/api/v1/relay/sessions/visit/client",
    });
    await manager.logout();
    expect(deps.closeTransports).toHaveBeenCalled();
    expect(deps.save).toHaveBeenLastCalledWith(null);
    await expect(manager.openVisit("remote", manager.snapshot().center)).rejects.toThrow();
  });

  it("lets a visit opened during a cold restore wait for this device's registration", async () => {
    const { manager, http } = fixture();
    const saved = {
      center: "https://aitapp.cn/api",
      token: "saved-jwt",
      expiresAt: Date.now() + 60_000,
      name: "Me",
    };
    await manager.restore(saved);
    // Saved hosts reconnect before the first registration tick has run.
    const visit = manager.openVisit("remote", saved.center);
    await vi.advanceTimersByTimeAsync(1);
    await expect(visit).resolves.toMatchObject({ relay_session_id: "visit" });
    const paths = http.mock.calls.map(([url]) => String(url));
    expect(paths.findIndex((path) => path.endsWith("/nodes/register"))).toBeLessThan(
      paths.findIndex((path) => path.endsWith("/relay-sessions")),
    );
    await manager.logout();
  });

  it("stops waiting for registration when the account signs out or the wait times out", async () => {
    const { manager, http } = fixture();
    const saved = {
      center: "https://aitapp.cn/api",
      token: "saved-jwt",
      expiresAt: Date.now() + 60_000,
      name: "Me",
    };
    const original = http.getMockImplementation()!;
    http.mockImplementation((url, options) =>
      String(url).endsWith("/nodes/register")
        ? Promise.resolve(Response.json({ error: { code: "unavailable" } }, { status: 503 }))
        : original(url, options),
    );
    await manager.restore(saved);
    const timedOut = manager.openVisit("remote", saved.center);
    const timedOutResult = expect(timedOut).rejects.toThrow("wait for this device to register");
    await vi.advanceTimersByTimeAsync(10_000);
    await timedOutResult;

    const signedOut = manager.openVisit("remote", saved.center);
    const signedOutResult = expect(signedOut).rejects.toThrow("wait for this device to register");
    await manager.logout();
    await signedOutResult;
    expect(http.mock.calls.some(([url]) => String(url).endsWith("/relay-sessions"))).toBe(false);
  });

  it("stops network activity in background and renews/re-registers an expired lease on resume", async () => {
    const { manager, http, deps, login, expireLease } = fixture();
    await login();
    await manager.select("remote");
    expect(manager.snapshot().stale).toBe(false);
    manager.suspend();
    expect(manager.snapshot().stale).toBe(true);
    const count = http.mock.calls.length;
    await vi.advanceTimersByTimeAsync(120_000);
    expect(http).toHaveBeenCalledTimes(count);
    await expect(manager.openVisit("remote", manager.snapshot().center)).rejects.toThrow();
    expect(deps.closeTransports).toHaveBeenCalled();
    expireLease();
    manager.resume();
    await vi.advanceTimersByTimeAsync(10);
    expect(http.mock.calls.filter(([url]) => String(url).endsWith("/nodes/register"))).toHaveLength(
      2,
    );
    expect(manager.snapshot()).toMatchObject({
      status: "online",
      stale: false,
      selected: { host_id: "remote" },
    });
    await manager.logout();
  });

  it("keeps registration backoff when recovery of an expired lease fails", async () => {
    const { manager, http, login, expireLease } = fixture();
    await login();
    const original = http.getMockImplementation()!;
    http.mockImplementation((url, options) =>
      String(url).endsWith("/nodes/register")
        ? Promise.resolve(
            Response.json({ error: { code: "registration_expired" } }, { status: 409 }),
          )
        : original(url, options),
    );
    expireLease();
    manager.resume();
    await vi.advanceTimersByTimeAsync(10);
    const registrations = () =>
      http.mock.calls.filter(([url]) => String(url).endsWith("/nodes/register"));
    expect(registrations()).toHaveLength(2);
    await vi.advanceTimersByTimeAsync(500);
    expect(registrations()).toHaveLength(2);
    expect(manager.snapshot().status).toBe("error");
    await manager.shutdown();
  });

  it("backs off when the service repeatedly rejects newly registered leases", async () => {
    const { manager, http, login, channel } = fixture();
    const original = http.getMockImplementation()!;
    http.mockImplementation((url, options) =>
      String(url).endsWith("/channel-tickets")
        ? Promise.resolve(
            Response.json({ error: { code: "node_session_expired" } }, { status: 401 }),
          )
        : original(url, options),
    );
    await login();
    await vi.advanceTimersByTimeAsync(500);
    expect(http.mock.calls.filter(([url]) => String(url).endsWith("/nodes/register"))).toHaveLength(
      2,
    );
    expect(channel.create).not.toHaveBeenCalled();
    expect(manager.snapshot().status).toBe("error");
    await manager.shutdown();
  });

  it("coalesces recovery requested during an in-flight tick and runs it without the polling delay", async () => {
    const { manager, http, login, channel } = fixture();
    const original = http.getMockImplementation()!;
    let resolve!: (value: Response) => void;
    http.mockImplementation((url, options) =>
      String(url).endsWith("/channel-tickets")
        ? new Promise((done) => {
            resolve = done;
          })
        : original(url, options),
    );
    await login();
    manager.resume();
    manager.refresh();
    await vi.advanceTimersByTimeAsync(1);
    expect(channel.create).not.toHaveBeenCalled();
    resolve(
      Response.json({
        channel_ticket: "fresh",
        channel_path: "/v1/relay/channel",
        protocol: "ait-channel-v1",
      }),
    );
    await vi.advanceTimersByTimeAsync(10);
    expect(channel.create).toHaveBeenCalledOnce();
    expect(http.mock.calls.filter(([url]) => String(url).endsWith("/renew"))).toHaveLength(1);
    expect(manager.snapshot().stale).toBe(false);
    await manager.shutdown();
  });

  it("replaces a silent foreground status channel without adding normal retry backoff", async () => {
    const { manager, login, channel } = fixture();
    await login();
    const old = channel.sockets[0]!;
    old.reply = false;
    manager.resume();
    await vi.advanceTimersByTimeAsync(2_000);
    manager.refresh();
    await vi.advanceTimersByTimeAsync(1_010);
    expect(old.transport.close).toHaveBeenCalledOnce();
    expect(channel.create).toHaveBeenCalledTimes(2);
    expect(manager.snapshot()).toMatchObject({ status: "online", stale: false });
    await manager.shutdown();
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
    const original = http.getMockImplementation()!;
    http.mockImplementation((url, options) =>
      String(url).endsWith("/auth/client/exchange")
        ? Promise.resolve(
            Response.json({
              access_token: "private-jwt",
              expires_in: 14 * 86400,
              user: { email: "me@example.test" },
            }),
          )
        : original(url, options),
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
    expect(
      http.mock.calls.filter(([url]) => String(url).endsWith("/auth/client/exchange")),
    ).toHaveLength(1);
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
    await next.loginWithBrowser("");
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
    const registration = http.mock.calls.find(([url]) => String(url).endsWith("/nodes/register"))!;
    expect(JSON.parse(String(registration[1]?.body)).runtime).toBeNull();
    const grant = await manager.publishHost(first);
    expect(grant).toEqual({
      center_url: "https://aitapp.cn/api",
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
    await manager.loginWithBrowser("https://other.test/api");
    await vi.advanceTimersByTimeAsync(22_000);
    expect(http.mock.calls).toContainEqual([
      "https://aitapp.cn/api/v1/node-sessions/lease-second/renew",
      expect.objectContaining({ method: "POST" }),
    ]);
    expect(await manager.publishHost(second)).toMatchObject({
      center_url: "https://aitapp.cn/api",
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

describe("account status channel", () => {
  it("uses a one-use ticket, retains the gateway and stops HTTP host polling", async () => {
    const { manager, login, http, channel } = fixture();
    await login();
    expect(http).toHaveBeenCalledWith(
      "https://aitapp.cn/api/v1/relay/channel-tickets",
      expect.objectContaining({
        headers: expect.objectContaining({ Authorization: "Bearer private-jwt" }),
        body: JSON.stringify({ source_node_session_id: "lease" }),
      }),
    );
    expect(channel.create).toHaveBeenCalledWith({
      url: "wss://aitapp.cn/api/v1/relay/channel",
      protocols: ["ait.ticket.one-use-channel"],
    });
    await vi.advanceTimersByTimeAsync(80_000);
    expect(channel.create).toHaveBeenCalledTimes(1);
    expect(channel.sockets[0]!.transport.send).toHaveBeenCalledWith(
      JSON.stringify({ type: "channel.heartbeat" }),
    );
    expect(http.mock.calls.some(([url]) => String(url).includes("/hosts/online"))).toBe(false);
    expect(manager.snapshot().stale).toBe(false);
    await manager.logout();
  });

  it("applies route events immediately while retaining offline and unbound hosts", async () => {
    const { manager, login, channel, host } = fixture();
    await login();
    await manager.select("remote");
    const socket = channel.sockets[0]!;
    const offline = {
      ...host,
      online: false,
      instance_id: null,
      relay_modes: [],
      offline_since: "2026-10-01T00:00:00Z",
    };
    const manual = { ...offline, host_id: "manual", node_id: null, server_id: null };
    socket.receive({ type: "hosts.snapshot", hosts: [offline, manual], observed_at: "old" });
    expect(manager.snapshot().hosts).toEqual([offline, manual]);
    expect(manager.snapshot().selected).toEqual(offline);
    await expect(manager.select("remote")).rejects.toThrow("offline");
    socket.receive({
      type: "node.online",
      node_id: "other",
      node_session_id: "s",
      host_id: "remote",
      host,
      observed_at: "old",
    });
    expect(manager.snapshot().hosts.find((entry) => entry.host_id === "remote")).toEqual(host);
    expect(manager.snapshot().selected).toEqual(host);
    await expect(manager.select("remote")).resolves.toBeDefined();
    socket.receive({
      type: "node.offline",
      node_id: "other",
      node_session_id: "s",
      host_id: "remote",
      observed_at: "old",
    });
    expect(manager.snapshot().hosts.find((entry) => entry.host_id === "remote")).toMatchObject({
      online: false,
      instance_id: null,
    });
    await expect(manager.select("remote")).rejects.toThrow("offline");
    // A previously unknown Host must also become selectable from the event alone.
    const added = { ...host, host_id: "new-host", node_id: "new-node" };
    socket.receive({
      type: "node.online",
      node_id: added.node_id,
      node_session_id: "new-session",
      host_id: added.host_id,
      host: added,
      observed_at: "new",
    });
    expect(manager.snapshot().hosts.find((entry) => entry.host_id === added.host_id)).toEqual(
      added,
    );
    await expect(manager.select(added.host_id)).resolves.toBeDefined();
    socket.receive({
      type: "node.online",
      node_id: added.node_id,
      node_session_id: "new-session",
      host_id: added.host_id,
      host: added,
      observed_at: "new",
    });
    expect(
      manager.snapshot().hosts.filter((entry) => entry.host_id === added.host_id),
    ).toHaveLength(1);
    socket.receive({ type: "hosts.snapshot", hosts: [], observed_at: "old" });
    expect(manager.snapshot()).toMatchObject({ hosts: [], selected: null });
    await manager.logout();
  });

  it("marks disconnection stale and obtains a fresh ticket before replacing the cache", async () => {
    const { manager, login, channel, http, host } = fixture();
    await login();
    await manager.select("remote");
    const socket = channel.sockets[0]!;
    socket.disconnect();
    expect(manager.snapshot()).toMatchObject({ stale: true, hosts: [host], selected: host });
    socket.receive({ type: "hosts.snapshot", hosts: [], observed_at: "old" });
    expect(manager.snapshot().hosts).toEqual([host]);
    await vi.advanceTimersByTimeAsync(4000);
    expect(channel.create).toHaveBeenCalledTimes(2);
    expect(
      http.mock.calls.filter(([url]) => String(url).endsWith("/channel-tickets")),
    ).toHaveLength(2);
    expect(manager.snapshot().stale).toBe(false);
    await manager.logout();
    channel.sockets[1]!.snapshot();
    await vi.advanceTimersByTimeAsync(60_000);
    expect(manager.snapshot()).toMatchObject({ status: "logged_out", hosts: [] });
    expect(channel.create).toHaveBeenCalledTimes(2);
  });

  it("does not open a delayed ticket after suspension and reconnects after lease renewal", async () => {
    const { manager, login, http, channel } = fixture();
    const original = http.getMockImplementation()!;
    let resolve!: (value: Response) => void;
    http.mockImplementation((url, options) =>
      String(url).endsWith("/channel-tickets")
        ? new Promise((done) => {
            resolve = done;
          })
        : original(url, options),
    );
    await login();
    manager.suspend();
    resolve(
      Response.json({
        channel_ticket: "late",
        channel_path: "/v1/relay/channel",
        protocol: "ait-channel-v1",
      }),
    );
    await vi.advanceTimersByTimeAsync(1);
    expect(channel.create).not.toHaveBeenCalled();
    http.mockImplementation(original);
    manager.resume();
    await vi.advanceTimersByTimeAsync(1);
    expect(channel.create).toHaveBeenCalledTimes(1);
    expect(manager.snapshot().stale).toBe(false);
    await manager.shutdown();
  });

  it.each([
    "https://foreign.test/v1/relay/channel",
    "//foreign.test/v1/relay/channel",
    "/v1/relay/control",
    "/v1/relay/channel?ticket=x",
  ])("rejects unsafe or unsupported ticket path %s", async (path) => {
    const { manager, login, http, channel } = fixture();
    const original = http.getMockImplementation()!;
    http.mockImplementation((url, options) =>
      String(url).endsWith("/channel-tickets")
        ? Promise.resolve(
            Response.json({
              channel_ticket: "ticket",
              channel_path: path,
              protocol: "ait-channel-v1",
            }),
          )
        : original(url, options),
    );
    await login();
    expect(channel.create).not.toHaveBeenCalled();
    expect(manager.snapshot().stale).toBe(true);
    await manager.logout();
  });
});

it.each(["disconnect", "suspend", "logout"])(
  "invalidates individually confirmed Hosts on %s before a full snapshot",
  async (action) => {
    const { manager, login, channel, host } = fixture(false);
    await login();
    const socket = channel.sockets[0]!;
    socket.receive(welcome);
    const online = {
      type: "node.online",
      node_id: host.node_id,
      node_session_id: "s",
      host_id: host.host_id,
      host,
      observed_at: "now",
    };
    socket.receive(online);
    expect(manager.snapshot()).toMatchObject({
      status: "connecting",
      stale: true,
      confirmedHostIds: [host.host_id],
    });
    manager.snapshot().confirmedHostIds!.push("untrusted");
    expect(manager.snapshot().confirmedHostIds).toEqual([host.host_id]);
    if (action === "disconnect") socket.disconnect();
    else if (action === "suspend") manager.suspend();
    else await manager.logout();
    expect(manager.snapshot()).toMatchObject({ stale: true, confirmedHostIds: [] });
    socket.receive(online);
    expect(manager.snapshot().confirmedHostIds).toEqual([]);
    await manager.logout();
  },
);

describe("hub tickets", () => {
  const hubGrant = { hub_ticket: "c".repeat(64), hub_path: "/v1/hub", protocol: "ait-hub-v1" };
  const builtIn = {
    serverId: "first",
    instanceId: "instance-1",
    name: "Desktop",
    platform: "linux",
  };

  function hubFixture(grant: object = hubGrant) {
    const context = fixture();
    const original = context.http.getMockImplementation()!;
    context.http.mockImplementation((url, options) =>
      String(url).endsWith("/hub-tickets")
        ? Promise.resolve(Response.json(grant))
        : original(url, options),
    );
    return context;
  }

  const hubCalls = (http: ReturnType<typeof fixture>["http"]) =>
    http.mock.calls.filter(([url]) => String(url).endsWith("/hub-tickets"));

  it("issues an access ticket for this device's node session", async () => {
    const { manager, http, login } = hubFixture();
    await expect(manager.openHubTicket()).rejects.toThrow("Sign in");
    await login();
    await expect(manager.openHubTicket()).resolves.toEqual({
      centerUrl: "https://aitapp.cn/api",
      ticket: "c".repeat(64),
    });
    const [url, options] = hubCalls(http)[0]!;
    expect(url).toBe("https://aitapp.cn/api/v1/node-sessions/lease/hub-tickets");
    expect(options).toMatchObject({
      method: "POST",
      body: JSON.stringify({ ep: "access" }),
      headers: { Authorization: "Bearer private-jwt" },
    });
    await manager.logout();
  });

  it("rejects a grant for another protocol", async () => {
    const { manager, login } = hubFixture({ ...hubGrant, protocol: "ait-hub-v2" });
    await login();
    await expect(manager.openHubTicket()).rejects.toThrow("invalid hub ticket");
    await manager.logout();
  });

  function desktopHub(localHub: AccountDependencies["localHub"]) {
    const context = hubFixture();
    context.deps.publishRuntime = false;
    context.deps.runtime = () => ({
      status: "running",
      serverId: "first",
      instanceId: "instance-1",
    });
    context.deps.localHub = localHub;
    return context;
  }

  it("brings the built-in daemon's serve link online alongside the relay control", async () => {
    let online = false;
    const localHub = vi.fn(async (method: string) =>
      method === "GET" ? { online, connecting: false, link: null, error: null } : null,
    );
    const { manager, http, deps, login } = desktopHub(localHub);
    await login();
    await manager.publishHost(builtIn);
    await vi.advanceTimersByTimeAsync(1);
    const [url, options] = hubCalls(http)[0]!;
    expect(url).toBe("https://aitapp.cn/api/v1/node-sessions/lease-first/hub-tickets");
    expect(options).toMatchObject({ body: JSON.stringify({ ep: "serve" }) });
    expect(localHub).toHaveBeenCalledWith("PUT", {
      center_url: "https://aitapp.cn/api",
      hub_ticket: "c".repeat(64),
      node_session_id: "lease-first",
    });
    online = true;
    await vi.advanceTimersByTimeAsync(40_000);
    expect(hubCalls(http)).toHaveLength(1);
    // The legacy relay control path is untouched.
    expect(deps.local).not.toHaveBeenCalledWith("PUT", expect.anything());
    await manager.unpublishHost("first");
    expect(localHub).toHaveBeenLastCalledWith("DELETE");
    await manager.logout();
  });

  it.each([404, 409])("stops quietly when the local hub API answers %i", async (status) => {
    const localHub = vi.fn(async (_method: string) => {
      throw Object.assign(new Error("unavailable"), { status });
    });
    const { manager, http, deps, login } = desktopHub(localHub);
    await login();
    await manager.publishHost(builtIn);
    await vi.advanceTimersByTimeAsync(40_000);
    expect(localHub).toHaveBeenCalledOnce();
    expect(hubCalls(http)).toHaveLength(0);
    expect(manager.snapshot().error).toBeNull();
    // A restarted runtime instance is a new publication and is asked again.
    deps.runtime = () => ({ status: "running", serverId: "first", instanceId: "instance-2" });
    await manager.publishHost({ ...builtIn, instanceId: "instance-2" });
    await vi.advanceTimersByTimeAsync(40_000);
    expect(localHub.mock.calls.filter(([method]) => method === "GET")).toHaveLength(2);
    await manager.unpublishHost("first");
    await manager.logout();
  });

  it("never delays lease renewal while the local hub API hangs", async () => {
    const localHub = vi.fn((method: string) =>
      method === "GET" ? new Promise<never>(() => undefined) : Promise.resolve(null),
    );
    const { manager, http, login } = desktopHub(localHub);
    await login();
    await manager.publishHost(builtIn);
    const renewals = () => http.mock.calls.filter(([url]) => String(url).endsWith("/renew")).length;
    const before = renewals();
    await vi.advanceTimersByTimeAsync(45_000);
    expect(renewals()).toBeGreaterThan(before);
    expect(localHub).toHaveBeenCalledOnce();
    expect(localHub).toHaveBeenCalledWith("GET");
    await manager.unpublishHost("first");
    await manager.logout();
  });

  it("backs off when the center has no hub", async () => {
    const localHub = vi.fn(async () => ({ online: false, connecting: false }));
    const { manager, http, login } = desktopHub(localHub);
    const original = http.getMockImplementation()!;
    http.mockImplementation((url, options) =>
      String(url).endsWith("/hub-tickets")
        ? Promise.resolve(Response.json({ error: { code: "not_found" } }, { status: 404 }))
        : original(url, options),
    );
    await login();
    await manager.publishHost(builtIn);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(hubCalls(http)).toHaveLength(1);
    expect(localHub).not.toHaveBeenCalledWith("PUT", expect.anything());
    await manager.unpublishHost("first");
    await manager.logout();
  });
});

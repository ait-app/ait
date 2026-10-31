import { channelFixture, welcome } from "../test/account-channel.js";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  AccountSessionManager,
  accountSubject,
  normalizeCenter,
  type AccountDependencies,
  type SavedAccount,
} from "./account-session.js";
import { createHash } from "node:crypto";
import { hubFixture } from "../test/hub.js";

afterEach(() => vi.useRealTimers());

describe("account deletion", () => {
  it("deletes the signed-in account before clearing credentials, connections and daemon leases", async () => {
    const f = fixture();
    await f.login();
    f.http.mockClear();
    vi.mocked(f.deps.save).mockClear();
    vi.mocked(f.deps.closeTransports).mockClear();
    let finish!: (response: Response) => void;
    f.http.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const pending = f.manager.deleteAccount();
    expect(f.http).toHaveBeenCalledExactlyOnceWith(
      "https://aitapp.cn/api/v1/me",
      expect.objectContaining({
        method: "DELETE",
        headers: expect.objectContaining({ Authorization: "Bearer private-jwt" }),
      }),
    );
    expect(f.http.mock.calls[0]?.[1]?.body).toBeUndefined();
    expect(f.deps.save).not.toHaveBeenCalled();
    expect(f.deps.closeTransports).not.toHaveBeenCalled();
    expect(f.manager.snapshot().status).toBe("online");
    finish(new Response(null, { status: 204 }));
    await expect(pending).resolves.toBe(true);
    expect(f.deps.save).toHaveBeenLastCalledWith(null);
    expect(f.deps.closeTransports).toHaveBeenCalledOnce();
    expect(f.manager.snapshot()).toMatchObject({
      status: "logged_out",
      hosts: [],
      selected: null,
      synchronizedHosts: [],
    });
    f.http.mockClear();
    await vi.advanceTimersByTimeAsync(30_000);
    expect(f.http).not.toHaveBeenCalled();
  });

  it("uses the signed-in custom service address", async () => {
    const f = fixture();
    await f.manager.restore({
      center: "https://custom.test/api",
      token: "saved-jwt",
      name: "Me",
      expiresAt: Date.now() + 60_000,
    });
    await f.manager.deleteAccount();
    expect(f.http).toHaveBeenCalledExactlyOnceWith(
      "https://custom.test/api/v1/me",
      expect.objectContaining({
        method: "DELETE",
        headers: expect.objectContaining({ Authorization: "Bearer saved-jwt" }),
      }),
    );
  });

  it.each(["server", "network"])("preserves the session after a %s failure", async (failure) => {
    const f = fixture();
    await f.login();
    const snapshot = f.manager.snapshot();
    vi.mocked(f.deps.save).mockClear();
    vi.mocked(f.deps.closeTransports).mockClear();
    if (failure === "server")
      f.http.mockResolvedValueOnce(
        Response.json(
          { error: { code: "unavailable", message: "Try again later" } },
          { status: 503 },
        ),
      );
    else f.http.mockRejectedValueOnce(new Error("Network unavailable"));
    await expect(f.manager.deleteAccount()).rejects.toThrow(
      failure === "server" ? "Try again later" : "Network unavailable",
    );
    expect(f.manager.snapshot()).toEqual(snapshot);
    expect(f.deps.save).not.toHaveBeenCalled();
    expect(f.deps.closeTransports).not.toHaveBeenCalled();
    await f.manager.logout();
  });

  it("opens browser confirmation when the service requires it and keeps the session", async () => {
    const f = fixture();
    const openUrl = vi.fn(async () => undefined);
    f.deps.openUrl = openUrl;
    await f.manager.restore({
      center: "https://custom.test/api",
      token: "saved-jwt",
      name: "Me",
      expiresAt: Date.now() + 60_000,
    });
    vi.mocked(f.deps.save).mockClear();
    vi.mocked(f.deps.closeTransports).mockClear();
    f.http.mockResolvedValueOnce(
      Response.json(
        { error: { code: "browser_confirmation_required", message: "Use the browser" } },
        { status: 403 },
      ),
    );
    await expect(f.manager.deleteAccount()).resolves.toBe(false);
    expect(openUrl).toHaveBeenCalledExactlyOnceWith(
      "https://custom.test/api/v1/auth/delete/authorize",
    );
    expect(f.deps.save).not.toHaveBeenCalled();
    expect(f.deps.closeTransports).not.toHaveBeenCalled();
    expect(f.manager.snapshot()).toMatchObject({ status: "connecting", name: "Me" });
    await f.manager.logout();
  });

  it("reports browser confirmation as an error when no browser is available", async () => {
    const f = fixture();
    await f.login();
    f.http.mockResolvedValueOnce(
      Response.json(
        { error: { code: "browser_confirmation_required", message: "Use the browser" } },
        { status: 403 },
      ),
    );
    await expect(f.manager.deleteAccount()).rejects.toThrow("Use the browser");
    expect(f.manager.snapshot().status).not.toBe("logged_out");
    await f.manager.logout();
  });

  it("requires a signed-in account and ignores a delayed response after an account switch", async () => {
    const f = fixture();
    await expect(f.manager.deleteAccount()).rejects.toThrow("Sign in first");
    expect(f.http).not.toHaveBeenCalled();
    await f.login();
    let finish!: (response: Response) => void;
    f.http.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const pending = f.manager.deleteAccount();
    await f.manager.logout();
    await f.manager.restore({
      center: "https://other.test/api",
      token: "other-jwt",
      name: "Other",
      expiresAt: Date.now() + 60_000,
    });
    const rejected = expect(pending).rejects.toThrow("The account changed");
    finish(new Response(null, { status: 204 }));
    await rejected;
    expect(f.manager.snapshot()).toMatchObject({ status: "connecting", name: "Other" });
    await f.manager.logout();
  });
});

describe("account information", () => {
  it("reads the login identity and account expiry without exposing credentials", async () => {
    const f = fixture();
    await f.login();
    f.http.mockClear();
    f.http.mockResolvedValueOnce(
      Response.json({
        email: "me@example.test",
        display_name: "Nickname",
        expires_at: "2027-01-01T00:00:00Z",
        auth_subject: "private-subject",
      }),
    );
    expect(await f.manager.getAccountInfo()).toEqual({
      loginName: "me@example.test",
      expiresAt: "2027-01-01T00:00:00Z",
    });
    expect(f.http).toHaveBeenCalledExactlyOnceWith(
      "https://aitapp.cn/api/v1/me",
      expect.objectContaining({
        headers: expect.objectContaining({ Authorization: "Bearer private-jwt" }),
      }),
    );
    await f.manager.logout();
  });

  it("supports phone-only accounts with unlimited access", async () => {
    const f = fixture();
    await f.login();
    f.http.mockResolvedValueOnce(
      Response.json({
        phone_number: "+8613800000000",
        expires_at: null,
      }),
    );
    expect(await f.manager.getAccountInfo()).toEqual({
      loginName: "+8613800000000",
      expiresAt: null,
    });
    await f.manager.logout();
  });

  it("rejects information from a previous account after logout", async () => {
    const f = fixture();
    await f.login();
    let finish!: (value: Response) => void;
    f.http.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const pending = f.manager.getAccountInfo();
    await f.manager.logout();
    const rejected = expect(pending).rejects.toThrow("The account changed");
    finish(Response.json({ email: "old@example.test", expires_at: null }));
    await rejected;
    await expect(f.manager.getAccountInfo()).rejects.toThrow("Sign in first");
  });

  it("rejects an invalid account expiry", async () => {
    const f = fixture();
    await f.login();
    f.http.mockResolvedValueOnce(
      Response.json({ email: "me@example.test", expires_at: "invalid" }),
    );
    await expect(f.manager.getAccountInfo()).rejects.toThrow("invalid account information");
    await f.manager.logout();
  });
});

it("discovers a Hub-only Host independently of its offline Relay route", async () => {
  const f = fixture();
  Object.assign(f.host, { online: false, instance_id: null, relay_modes: [] });
  const h = hubFixture({
    ticket: async () => ({ centerUrl: "https://aitapp.cn/api", ticket: "a".repeat(64) }),
  });
  f.manager.attachHub(h.link);
  await f.login();
  const socket = await h.online([
    {
      node_id: "other",
      host_id: "remote",
      name: "Remote",
      platform: "linux",
      app_version: "test",
      state: "online",
    },
  ]);
  expect(f.manager.snapshot().hosts[0]).toMatchObject({
    online: true,
    hub_online: true,
    instance_id: null,
    relay_modes: [],
  });
  expect(f.deps.notify).toHaveBeenLastCalledWith(f.manager.snapshot());
  // An old Relay snapshot must not override current Hub availability.
  f.channel.sockets[0]!.receive({ type: "hosts.snapshot", hosts: [f.host], observed_at: "now" });
  expect(f.manager.snapshot().hosts[0]?.online).toBe(true);
  socket.presence(2, []);
  expect(f.manager.snapshot().hosts[0]).toMatchObject({ online: false });
  h.link.stop();
  await f.manager.logout();
});

it("does not import presence from a different account center", async () => {
  const f = fixture();
  f.host.online = false;
  const h = hubFixture();
  f.manager.attachHub(h.link);
  await f.login();
  await h.online([
    {
      node_id: "other",
      host_id: "remote",
      name: "Remote",
      platform: "linux",
      app_version: "test",
      state: "online",
    },
  ]);
  expect(f.manager.snapshot().hosts[0]?.online).toBe(false);
  h.link.stop();
  await f.manager.logout();
});

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
      const visit = await manager.openDownload("remote", "download-token", saved.center);
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
    if (path.endsWith("/relay-downloads"))
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
  it("keeps host downloads available when another host is selected or deselected", async () => {
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
    await manager.openDownload("remote", "download-token", center);
    await manager.select("another");
    await manager.openDownload("remote", "download-token", center);
    await manager.openDownload("another", "download-token", center);
    await manager.select(null);
    await manager.openDownload("remote", "download-token", center);
    expect(deps.closeTransports).not.toHaveBeenCalled();
    await manager.logout();
    expect(deps.closeTransports).toHaveBeenCalledOnce();
  });

  it("rejects saved hosts from another service before sending account credentials", async () => {
    const { manager, login, http } = fixture();
    await login();
    const count = http.mock.calls.length;
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
    await vi.advanceTimersByTimeAsync(20_000);
    expect(http.mock.calls.some(([url]) => String(url).endsWith("/renew"))).toBe(true);
    expect(JSON.stringify(manager.snapshot())).not.toContain("private-jwt");
    expect(JSON.stringify(vi.mocked(deps.save).mock.calls)).not.toContain(" secret ");
    await manager.logout();
  });

  it("allows downloads without a new selection and revokes them on logout", async () => {
    const { manager, deps, login } = fixture();
    await login();
    expect(
      await manager.openDownload("remote", "download-token", manager.snapshot().center),
    ).toMatchObject({
      url: "wss://aitapp.cn/api/v1/relay/sessions/visit/client",
    });
    await manager.logout();
    expect(deps.closeTransports).toHaveBeenCalled();
    expect(deps.save).toHaveBeenLastCalledWith(null);
    await expect(
      manager.openDownload("remote", "download-token", manager.snapshot().center),
    ).rejects.toThrow();
  });

  it("lets a download opened during a cold restore wait for this device's registration", async () => {
    const { manager, http } = fixture();
    const saved = {
      center: "https://aitapp.cn/api",
      token: "saved-jwt",
      expiresAt: Date.now() + 60_000,
      name: "Me",
    };
    await manager.restore(saved);
    // Saved hosts reconnect before the first registration tick has run.
    const visit = manager.openDownload("remote", "download-token", saved.center);
    await vi.advanceTimersByTimeAsync(1);
    await expect(visit).resolves.toMatchObject({ relay_session_id: "visit" });
    const paths = http.mock.calls.map(([url]) => String(url));
    expect(paths.findIndex((path) => path.endsWith("/nodes/register"))).toBeLessThan(
      paths.findIndex((path) => path.endsWith("/relay-downloads")),
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
    const timedOut = manager.openDownload("remote", "download-token", saved.center);
    const timedOutResult = expect(timedOut).rejects.toThrow("wait for this device to register");
    await vi.advanceTimersByTimeAsync(10_000);
    await timedOutResult;

    const signedOut = manager.openDownload("remote", "download-token", saved.center);
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
    await expect(
      manager.openDownload("remote", "download-token", manager.snapshot().center),
    ).rejects.toThrow();
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
});

describe("accountSubject", () => {
  it("reads the token subject and ignores malformed tokens", () => {
    const payload = btoa(JSON.stringify({ sub: "user-1", exp: 1 }))
      .replace(/\+/g, "-")
      .replace(/\//g, "_")
      .replace(/=+$/, "");
    expect(accountSubject(`header.${payload}.signature`)).toBe("user-1");
    expect(accountSubject("not-a-jwt")).toBeUndefined();
    expect(accountSubject("a.!!!.c")).toBeUndefined();
  });
});

describe("daemon authorization handoff", () => {
  const runtime = {
    serverId: "00000000-0000-4000-8000-000000000001",
    instanceId: "instance",
    name: "SHLP",
    platform: "darwin",
  };
  function approvalFixture() {
    const context = fixture();
    context.deps.randomUUID = () => "00000000-0000-4000-8000-000000000002";
    const original = context.http.getMockImplementation()!;
    context.http.mockImplementation((url, options) =>
      String(url).endsWith("/auth/device/authorize")
        ? Promise.resolve(
            Response.json({
              device_code: "a".repeat(64),
              user_code: "ABCD-0123-ABCD",
              expires_in: 600,
              interval: 5,
            }),
          )
        : original(url, options),
    );
    return context;
  }

  it("approves a machine once and delegates token consumption, registration and renewal to the daemon", async () => {
    const { manager, http, login } = approvalFixture();
    await login();
    const handoff = await manager.authorizeHost(runtime);
    expect(handoff).toMatchObject({
      kind: "device_code",
      device_code: "a".repeat(64),
      user_code: "ABCD-0123-ABCD",
      interval: 5,
    });
    const calls = http.mock.calls;
    const authorization = calls.find(([url]) => String(url).endsWith("/auth/device/authorize"))!;
    expect(JSON.parse(String(authorization[1]?.body))).toEqual({
      client_id: "ait-daemon",
      machine: {
        server_id: runtime.serverId,
        display_name: "SHLP",
        platform: "darwin",
        app_version: "test",
      },
    });
    expect(authorization[1]?.headers).not.toHaveProperty("Authorization");
    expect(calls.find(([url]) => String(url).endsWith("/auth/device/approve"))?.[1]).toMatchObject({
      body: JSON.stringify({ user_code: handoff.user_code }),
      headers: { Authorization: "Bearer private-jwt" },
    });
    await vi.advanceTimersByTimeAsync(30_000);
    for (const [url, options] of http.mock.calls) {
      if (String(url).endsWith("/nodes/register"))
        expect(JSON.parse(String(options?.body)).runtime).toBeNull();
      expect(String(url)).not.toMatch(/device\/token|device\/refresh|control-tickets/);
      if (String(url).endsWith("/renew"))
        expect(String(url)).toContain("/node-sessions/lease/renew");
    }
    await manager.shutdown();
    expect(JSON.stringify(manager.snapshot())).not.toContain(handoff.device_code);
  });

  it("requires a signed-in official account and rejects invalid machine identity", async () => {
    const { manager, login, http } = approvalFixture();
    await expect(manager.authorizeHost(runtime)).rejects.toThrow("Sign in");
    await login();
    await expect(manager.authorizeHost({ ...runtime, serverId: "" })).rejects.toThrow(
      "Invalid host",
    );
    await manager.logout();
    await manager.restore({
      center: "https://custom.test/api",
      token: "custom-jwt",
      name: "Other",
      expiresAt: Date.now() + 60_000,
    });
    http.mockClear();
    await expect(manager.authorizeHost(runtime)).rejects.toThrow("official account service");
    expect(http).not.toHaveBeenCalled();
    await manager.logout();
  });

  it("does not approve or return a delayed authorization after sign-out", async () => {
    const { manager, login, http } = approvalFixture();
    await login();
    let finish!: (response: Response) => void;
    http.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const handoff = manager.authorizeHost(runtime);
    const rejected = expect(handoff).rejects.toThrow("account changed");
    await manager.logout();
    finish(
      Response.json({
        device_code: "a".repeat(64),
        user_code: "ABCD-0123-ABCD",
        expires_in: 600,
        interval: 5,
      }),
    );
    await rejected;
    expect(http.mock.calls.some(([url]) => String(url).endsWith("/auth/device/approve"))).toBe(
      false,
    );
  });
});

describe("Online Service membership", () => {
  const membership = {
    required: true,
    purchases_available: true,
    app_account_token: "018f20b0-fbcc-7000-8000-000000000001",
    product_id: "monthly",
    can_connect: false,
    status: "inactive",
    expires_at: null,
  };
  it("reads and verifies using the signed-in authority without sharing credentials with StoreKit", async () => {
    const f = fixture();
    await f.login();
    f.http.mockResolvedValueOnce(Response.json(membership));
    expect(await f.manager.getMembership()).toEqual(membership);
    const transaction = { transaction_id: "20000001", environment: "Production" as const };
    f.http.mockResolvedValueOnce(
      Response.json({ ...membership, can_connect: true, status: "active" }),
    );
    expect((await f.manager.verifyAppStoreTransaction(transaction)).can_connect).toBe(true);
    expect(f.http).toHaveBeenLastCalledWith(
      "https://aitapp.cn/api/v1/billing/app-store/transactions",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify(transaction),
        redirect: "error",
        headers: expect.objectContaining({ Authorization: "Bearer private-jwt" }),
      }),
    );
    await f.manager.logout();
  });
  it("submits Play proofs to the account authority and accepts both store options", async () => {
    const f = fixture();
    await f.login();
    const current = {
      ...membership,
      purchase_options: {
        app_store: { available: true, product_id: "monthly", base_plan_id: null },
        google_play: { available: true, product_id: "monthly", base_plan_id: "monthly" },
      },
      subscriptions: [
        {
          store: "google_play",
          status: "active",
          auto_renewing: true,
          expires_at: "2026-12-30T00:00:00Z",
        },
      ],
    };
    f.http.mockResolvedValueOnce(Response.json(current));
    const purchase = { purchase_token: "fixture-play-proof" };
    expect(await f.manager.verifyGooglePlayPurchase(purchase)).toEqual(current);
    expect(f.http).toHaveBeenLastCalledWith(
      "https://aitapp.cn/api/v1/billing/google-play/purchases",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify(purchase),
        headers: expect.objectContaining({ Authorization: "Bearer private-jwt" }),
      }),
    );
    for (const invalid of [
      { ...current, purchase_options: null },
      { ...current, purchase_options: { ...current.purchase_options, google_play: null } },
      { ...current, subscriptions: [{ ...current.subscriptions[0], expires_at: "invalid" }] },
    ]) {
      f.http.mockResolvedValueOnce(Response.json(invalid));
      await expect(f.manager.getMembership()).rejects.toThrow("invalid membership");
    }
    await f.manager.logout();
  });
  it("requires login, validates membership responses, and preserves purchase ownership errors", async () => {
    const f = fixture();
    await expect(f.manager.getMembership()).rejects.toThrow("Sign in first");
    await f.login();
    f.http.mockResolvedValueOnce(Response.json({ ...membership, app_account_token: "invalid" }));
    await expect(f.manager.getMembership()).rejects.toThrow("invalid membership");
    f.http.mockResolvedValueOnce(
      Response.json(
        {
          error: {
            code: "purchase_account_mismatch",
            message: "Sign in to the original Ait account",
          },
        },
        { status: 409 },
      ),
    );
    await expect(
      f.manager.verifyAppStoreTransaction({ transaction_id: "20000001", environment: "Sandbox" }),
    ).rejects.toMatchObject({ code: "purchase_account_mismatch", status: 409 });
    await f.manager.logout();
  });
});

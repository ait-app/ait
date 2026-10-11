import {
  accountLoginError,
  parseAccountCallback,
  type AccountBrowserLogin,
} from "./account-browser-login.js";

/** API base includes the management gateway prefix; all relay URLs derive from it. */
export const DEFAULT_ACCOUNT_CENTER = "https://dash.ait-app.com:8443/api";

export interface AccountHost {
  host_id: string;
  node_id: string;
  server_id: string;
  instance_id: string;
  name: string;
  platform: string;
  relay_modes: string[];
}

interface LoginUser {
  display_name?: string | null;
  email?: string | null;
  phone_number?: string | null;
  expires_at?: string | null;
}

interface NodeSession {
  node_id: string;
  node_session_id: string;
  host_id: string | null;
  control_required: boolean;
}

export interface SavedAccount {
  center: string;
  token: string;
  expiresAt: number;
  name: string;
  nodeSessionId?: string;
  accountExpiresAt?: string | null;
}

export interface AccountSnapshot {
  status: "logged_out" | "connecting" | "online" | "error";
  center: string;
  name: string;
  hostOnline: boolean;
  hosts: AccountHost[];
  stale: boolean;
  error: string | null;
  selected: AccountHost | null;
  /** Independently maintained daemon leases; contains no credentials. */
  synchronizedHosts?: string[];
  accountExpiresAt?: string | null;
  loginPending?: boolean;
}

export interface AccountDependencies {
  installationId: string;
  deviceName: string;
  platform: string;
  randomUUID(): string;
  appVersion: string;
  runtime(): { status: string; serverId: string; instanceId?: string; features?: string[] };
  local(method: "GET" | "PUT" | "DELETE", body?: unknown): Promise<unknown>;
  save(account: SavedAccount | null): Promise<void>;
  notify(snapshot: AccountSnapshot): void;
  closeTransports(): void;
  fetch?: typeof fetch;
  /** Desktop can keep login independent of explicitly publishing individual daemons. */
  publishRuntime?: boolean;
  browserLogin?: AccountBrowserLogin;
}

export interface AccountRuntime {
  serverId: string;
  instanceId: string;
  name: string;
  platform: string;
}

export interface HostControlGrant {
  center_url: string;
  control_ticket: string;
  node_session_id: string;
}

interface HostPublication {
  authority: SavedAccount;
  requests: Set<AbortController>;
  runtime: AccountRuntime;
  registrationId: string;
  installationId: string;
  node: NodeSession | null;
  nextRenew: number;
}

export class AccountError extends Error {
  constructor(
    message: string,
    readonly status: number,
    readonly code: string,
  ) {
    super(message);
  }
}

export function normalizeCenter(value: string): string {
  const url = new URL(value.trim() || DEFAULT_ACCOUNT_CENTER);
  const loopback = ["localhost", "127.0.0.1", "[::1]"].includes(url.hostname);
  if (
    (url.protocol !== "https:" && !(url.protocol === "http:" && loopback)) ||
    url.username ||
    url.password ||
    url.search ||
    url.hash
  ) {
    throw new Error("The service URL must use HTTPS. HTTP is allowed only for local development.");
  }
  return url.toString().replace(/\/+$/, "");
}

function retryDelay(failures: number): number {
  return Math.min(30_000, 1000 * 2 ** Math.min(failures, 5)) * (0.8 + Math.random() * 0.4);
}

/** Platform-owned account authority; runtime connectors receive short-lived tickets only. */
export class AccountSessionManager {
  private browserLogin: AbortController | null = null;
  private account: SavedAccount | null = null;
  private suspended = false;
  private node: NodeSession | null = null;
  private registrationId: string;
  private runtimeInstance: string | null = null;
  private generation = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private running: Promise<void> | null = null;
  private requests = new Set<AbortController>();
  private nextRenew = 0;
  private nextDiscovery = 0;
  private discoveryFailures = 0;
  private controlFailures = 0;
  private nextControl = 0;
  private nextRegistration = 0;
  private registrationFailures = 0;
  private publications = new Map<string, HostPublication>();
  private closing = false;
  private state: AccountSnapshot = {
    status: "logged_out",
    center: DEFAULT_ACCOUNT_CENTER,
    name: "",
    hostOnline: false,
    hosts: [],
    stale: true,
    error: null,
    selected: null,
  };

  constructor(private readonly deps: AccountDependencies) {
    this.registrationId = deps.randomUUID();
  }

  snapshot(): AccountSnapshot {
    const cloneHost = (host: AccountHost): AccountHost => ({
      ...host,
      relay_modes: [...host.relay_modes],
    });
    return {
      ...this.state,
      hosts: this.state.hosts.map(cloneHost),
      selected: this.state.selected ? cloneHost(this.state.selected) : null,
      synchronizedHosts: [...this.publications.keys()],
    };
  }

  private update(patch: Partial<AccountSnapshot>): void {
    this.state = { ...this.state, ...patch };
    this.deps.notify(this.snapshot());
  }

  private async loginMethods(center: string): Promise<{ hosted: boolean }> {
    if (!this.deps.browserLogin) return { hosted: false };
    try {
      const options = await this.http<{
        authing_enabled?: boolean;
        native_login_enabled?: boolean;
      }>(normalizeCenter(center), null, "/v1/auth/providers", "GET");
      return { hosted: options.authing_enabled === true && options.native_login_enabled === true };
    } catch (error) {
      if (error instanceof AccountError && error.status === 404) return { hosted: false };
      throw error;
    }
  }

  cancelLogin(): void {
    this.browserLogin?.abort();
  }

  async loginWithBrowser(center: string): Promise<AccountSnapshot> {
    center = normalizeCenter(center);
    const browser = this.deps.browserLogin;
    if (!browser || !(await this.loginMethods(center)).hosted)
      throw new Error(
        "This service does not support client browser sign-in. Update the service or choose another service URL.",
      );
    await this.logout();
    const attempt = new AbortController();
    this.browserLogin = attempt;
    this.update({ loginPending: true, center });
    const requests = new Set<AbortController>();
    attempt.signal.addEventListener(
      "abort",
      () => {
        for (const request of requests) request.abort();
      },
      { once: true },
    );
    const timeout = setTimeout(() => attempt.abort(), 10 * 60_000);
    try {
      const state = browser.randomSecret();
      const verifier = browser.randomSecret();
      const challenge = await browser.challenge(verifier);
      if (attempt.signal.aborted) throw new Error("Sign-in cancelled.");
      const response = await browser.open(
        (redirectUri) => {
          const url = new URL(`${center}/v1/auth/authorize`);
          url.searchParams.set("redirect_uri", redirectUri);
          url.searchParams.set("state", state);
          url.searchParams.set("code_challenge", challenge);
          return url.toString();
        },
        state,
        attempt.signal,
      );
      if (attempt.signal.aborted) throw new Error("Sign-in cancelled.");
      const callback = parseAccountCallback(response.url, response.redirectUri, state);
      if (!callback) throw new Error("The sign-in callback is invalid. Start sign-in again.");
      const error = callback.searchParams.get("error");
      if (error) throw new AccountError(accountLoginError(error), 401, error);
      const result = await this.http<{
        access_token: string;
        expires_in: number;
        user: LoginUser;
      }>(
        center,
        null,
        "/v1/auth/client/exchange",
        "POST",
        {
          code: callback.searchParams.get("code"),
          code_verifier: verifier,
        },
        requests,
      );
      if (attempt.signal.aborted) throw new Error("Sign-in cancelled.");
      const snapshot = await this.acceptLogin(center, result);
      if (attempt.signal.aborted) {
        await this.logout();
        throw new Error("Sign-in cancelled.");
      }
      return { ...snapshot, loginPending: false };
    } catch (error) {
      if (attempt.signal.aborted) throw new Error("Sign-in cancelled or timed out. Try again.");
      throw error;
    } finally {
      clearTimeout(timeout);
      if (this.browserLogin === attempt) {
        this.browserLogin = null;
        this.update({ loginPending: false });
      }
    }
  }

  private async acceptLogin(
    center: string,
    result: {
      access_token: string;
      expires_in: number;
      user: LoginUser;
    },
  ): Promise<AccountSnapshot> {
    const contact = [result.user?.email, result.user?.phone_number].find(
      (value): value is string => typeof value === "string" && value.trim().length > 0,
    );
    if (
      !result.access_token ||
      !Number.isFinite(result.expires_in) ||
      result.expires_in <= 0 ||
      !contact
    )
      throw new Error("The service returned an invalid login session.");
    this.account = {
      center,
      token: result.access_token,
      expiresAt: Date.now() + result.expires_in * 1000,
      name:
        typeof result.user.display_name === "string" && result.user.display_name.trim()
          ? result.user.display_name.trim()
          : contact,
      accountExpiresAt: result.user.expires_at ?? null,
    };
    await this.deps.save(this.account);
    this.update({
      status: "connecting",
      center,
      name: this.account.name,
      accountExpiresAt: this.account.accountExpiresAt,
      error: null,
    });
    this.schedule(0);
    return this.snapshot();
  }

  async restore(account: SavedAccount): Promise<void> {
    if (this.account || account.expiresAt <= Date.now()) {
      return;
    }
    this.account = { ...account, center: normalizeCenter(account.center) };
    this.update({
      status: "connecting",
      center: this.account.center,
      name: account.name,
      accountExpiresAt: account.accountExpiresAt ?? null,
    });
    // A previous process activation is released before registering this process.
    // If the request cannot reach the center, normal registration waits for its lease.
    if (account.nodeSessionId) {
      await this.api(`/v1/node-sessions/${account.nodeSessionId}`, "DELETE").catch(() => undefined);
    }
    this.schedule(0);
  }

  async logout(): Promise<void> {
    await this.disconnectAccount(true);
  }

  private async disconnectAccount(forgetAccount: boolean): Promise<void> {
    this.cancelLogin();
    this.generation += 1;
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    for (const request of this.requests) request.abort();
    this.deps.closeTransports();
    const account = this.account;
    const node = this.node;
    const bindingServerId = this.deps.runtime().serverId;
    this.account = null;
    this.node = null;
    this.registrationId = this.deps.randomUUID();
    this.runtimeInstance = null;
    this.nextControl = this.nextDiscovery = this.nextRenew = this.nextRegistration = 0;
    this.controlFailures = this.discoveryFailures = this.registrationFailures = 0;
    await this.running?.catch(() => undefined);
    await this.deps.local("DELETE").catch(() => undefined);
    if (account && node) {
      await this.http(
        account.center,
        account.token,
        `/v1/node-sessions/${node.node_session_id}`,
        "DELETE",
      ).catch(() => undefined);
    }
    // Client logout releases only its binding daemon, never other host leases.
    await this.unpublishHost(bindingServerId).catch(() => undefined);
    const binding = this.publications.get(bindingServerId);
    if (binding) {
      for (const request of binding.requests) request.abort();
      this.publications.delete(bindingServerId);
    }
    if (forgetAccount) await this.deps.save(null);
    this.update({
      status: "logged_out",
      name: "",
      accountExpiresAt: null,
      hostOnline: false,
      hosts: [],
      stale: true,
      error: null,
      selected: null,
    });
    this.schedule(0);
  }

  async shutdown(): Promise<void> {
    this.closing = true;
    // Credentials were persisted at login/registration. Never erase and rewrite them on exit:
    // interruption or a failed write would otherwise turn closing the app into signing out.
    // restore() already handles releasing the saved activation, even if it was closed here.
    await this.disconnectAccount(false);
    for (const entry of this.publications.values())
      for (const request of entry.requests) request.abort();
  }

  async select(hostId: string | null): Promise<AccountSnapshot> {
    if (hostId === null) {
      this.update({ selected: null });
      return this.snapshot();
    }
    if (!this.account || !this.node)
      throw new Error("Sign in and wait for this device to register.");
    const host = this.state.hosts.find((host) => host.host_id === hostId);
    if (!host) throw new Error("The target host is offline. Refresh the host list.");
    if (this.state.selected?.host_id !== hostId) {
      this.update({ selected: host });
    }
    return this.snapshot();
  }

  refresh(): void {
    this.nextDiscovery = 0;
    this.schedule(0);
  }

  /** Register an explicitly chosen daemon; return only its one-use control grant. */
  async publishHost(runtime: AccountRuntime, needsGrant = true): Promise<HostControlGrant | null> {
    for (const value of [runtime.serverId, runtime.instanceId, runtime.name, runtime.platform]) {
      if (typeof value !== "string" || !value.trim() || value.length > 320)
        throw new Error("Invalid host identity.");
    }
    let entry = this.publications.get(runtime.serverId);
    if (this.closing || (!entry && (!this.account || this.suspended)))
      throw new Error("Sign in first.");
    if (entry && entry.runtime.instanceId !== runtime.instanceId) {
      const authority = entry.authority;
      await this.unpublishHost(runtime.serverId);
      entry = this.createPublication(runtime, authority);
    }
    if (!entry) {
      if (this.publications.size >= 16)
        throw new Error("Too many hosts synchronized with the service.");
      if (!this.account) throw new Error("Sign in first.");
      entry = this.createPublication(runtime, this.account);
    }
    if (!entry.node) {
      const node = await this.publicationApi<NodeSession>(entry, "/v1/nodes/register", "POST", {
        registration_id: entry.registrationId,
        installation_id: entry.installationId,
        display_name: runtime.name,
        platform: runtime.platform,
        app_version: this.deps.appVersion,
        runtime: {
          server_id: runtime.serverId,
          instance_id: runtime.instanceId,
          relay_modes: ["ait-rust-single-v1"],
        },
      }).catch((error: unknown) => {
        if (error instanceof AccountError && error.code === "registration_expired")
          entry.registrationId = this.deps.randomUUID();
        throw error;
      });
      if (this.publications.get(runtime.serverId) !== entry)
        throw new Error("Host synchronization cancelled.");
      entry.node = node;
      entry.nextRenew = Date.now() + 20_000;
      this.schedule(0);
    }
    if (!needsGrant) return null;
    const nodeSessionId = entry.node.node_session_id;
    const grant = await this.publicationApi<{ control_ticket: string }>(
      entry,
      `/v1/node-sessions/${nodeSessionId}/control-tickets`,
      "POST",
    );
    if (
      this.publications.get(runtime.serverId) !== entry ||
      entry.node?.node_session_id !== nodeSessionId
    )
      throw new Error("Host synchronization cancelled.");
    return {
      center_url: entry.authority.center,
      control_ticket: grant.control_ticket,
      node_session_id: nodeSessionId,
    };
  }

  /** Release one daemon's lease without signing out or affecting other hosts. */
  async unpublishHost(serverId: string): Promise<void> {
    const entry = this.publications.get(serverId);
    if (entry?.node)
      await this.publicationApi(entry, `/v1/node-sessions/${entry.node.node_session_id}`, "DELETE");
    if (this.publications.get(serverId) === entry) this.publications.delete(serverId);
  }

  private createPublication(runtime: AccountRuntime, authority: SavedAccount): HostPublication {
    const entry: HostPublication = {
      authority: { ...authority },
      requests: new Set(),
      runtime: { ...runtime },
      registrationId: this.deps.randomUUID(),
      // The center binds each host to one node, including after its lease is closed.
      // A daemon's stable UUID lets every publication reuse that node across clients/restarts.
      installationId: runtime.serverId,
      node: null,
      nextRenew: 0,
    };
    this.publications.set(runtime.serverId, entry);
    return entry;
  }

  private publicationApi<T = unknown>(
    entry: HostPublication,
    path: string,
    method: string,
    body?: unknown,
  ): Promise<T> {
    return this.http(
      entry.authority.center,
      entry.authority.token,
      path,
      method,
      body,
      entry.requests,
    );
  }

  private async renewPublications(): Promise<void> {
    const renewals = [...this.publications.values()].map(async (entry) => {
      if (!entry.node || Date.now() < entry.nextRenew) return;
      try {
        await this.publicationApi(
          entry,
          `/v1/node-sessions/${entry.node.node_session_id}/renew`,
          "POST",
        );
        entry.nextRenew = Date.now() + 20_000;
      } catch (error) {
        if (this.publications.get(entry.runtime.serverId) !== entry) return;
        if (
          error instanceof AccountError &&
          ["node_session_expired", "registration_expired"].includes(error.code)
        ) {
          entry.node = null;
          entry.registrationId = this.deps.randomUUID();
        } else {
          entry.nextRenew =
            Date.now() + (error instanceof AccountError && error.status === 401 ? 30_000 : 2000);
        }
      }
    });
    await Promise.all(renewals);
  }

  openVisit(hostId: string, center: string) {
    return this.openSession(hostId, center, "ait-rust-single-v1");
  }

  openDownload(hostId: string, token: string, center: string) {
    return this.openSession(hostId, center, "ait-download-v1", token);
  }

  private async openSession(
    hostId: string,
    expectedCenter: string,
    mode: string,
    token?: string,
  ): Promise<{
    relay_session_id: string;
    client_ticket: string;
    server_id: string;
    instance_id: string;
    url: string;
  }> {
    if (this.suspended || !this.account || !this.node) {
      throw new Error("Sign in and wait for this device to register.");
    }
    if (normalizeCenter(expectedCenter) !== this.account.center) {
      throw new Error("Sign in to the online service used by this host.");
    }
    const generation = this.generation;
    const center = this.account.center;
    const result = await this.api<{
      relay_session_id: string;
      client_ticket: string;
      server_id: string;
      instance_id: string;
    }>(
      `/v1/hosts/${hostId}/${token === undefined ? "relay-sessions" : "relay-downloads"}`,
      "POST",
      {
        source_node_session_id: this.node.node_session_id,
        attempt_id: this.deps.randomUUID(),
        mode,
        ...(token === undefined ? {} : { download_token: token }),
      },
    );
    if (generation !== this.generation) {
      await this.closeVisit(result.relay_session_id);
      throw new Error("Connection cancelled.");
    }
    const url = new URL(`${center}/v1/relay/sessions/${result.relay_session_id}/client`);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    return { ...result, url: url.toString() };
  }

  async closeVisit(id: string): Promise<void> {
    if (this.account) await this.api(`/v1/relay/sessions/${id}`, "DELETE").catch(() => undefined);
  }

  /** Stop network activity while a native client is backgrounded; keep its saved login. */
  suspend(): void {
    this.suspended = true;
    this.generation += 1;
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    for (const request of this.requests) request.abort();
    this.deps.closeTransports();
    this.schedule(0);
  }

  /** Revalidate the node lease and discover hosts when the app becomes active. */
  resume(): void {
    this.suspended = false;
    this.nextRenew = 0;
    this.refresh();
  }

  private schedule(delay: number): void {
    if (this.closing || ((!this.account || this.suspended) && !this.publications.size)) return;
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = null;
      if (this.running) {
        this.schedule(1000);
        return;
      }
      this.running = this.tick().finally(() => {
        this.running = null;
        this.schedule(2000);
      });
    }, delay);
    this.timer.unref?.();
  }

  private async tick(): Promise<void> {
    // Host lease authority survives client sign-out and client-node failures.
    await this.renewPublications();
    if (!this.account || this.suspended || this.closing) return;
    const generation = this.generation;
    try {
      if (this.account.expiresAt <= Date.now())
        throw new AccountError("Your session has expired. Sign in again.", 401, "unauthorized");
      const runtime = this.deps.runtime();
      const ready =
        this.deps.publishRuntime !== false &&
        runtime.status === "running" &&
        runtime.instanceId &&
        runtime.features?.includes("ait-rust-single-v1");
      const instance = ready ? runtime.instanceId! : null;
      if (this.node && this.runtimeInstance && this.runtimeInstance !== instance) {
        this.deps.closeTransports();
        await this.deps.local("DELETE").catch(() => undefined);
        await this.api(`/v1/node-sessions/${this.node.node_session_id}`, "DELETE");
        this.node = null;
        this.registrationId = this.deps.randomUUID();
        this.update({ hostOnline: false });
      }
      if (!this.node || (!this.runtimeInstance && instance)) {
        if (Date.now() < this.nextRegistration) return;
        const node = await this.api<NodeSession>("/v1/nodes/register", "POST", {
          registration_id: this.registrationId,
          installation_id: this.deps.installationId,
          display_name: this.deps.deviceName,
          platform: this.deps.platform,
          app_version: this.deps.appVersion,
          runtime: instance
            ? {
                server_id: runtime.serverId,
                instance_id: instance,
                relay_modes: ["ait-rust-single-v1"],
              }
            : null,
        });
        if (generation !== this.generation || !this.account) return;
        this.node = node;
        this.registrationFailures = 0;
        this.nextRegistration = 0;
        this.runtimeInstance = instance;
        this.account.nodeSessionId = node.node_session_id;
        await this.deps.save(this.account);
        this.nextRenew = Date.now() + 20_000;
        this.nextDiscovery = 0;
        this.update({ status: "online", error: null });
      }
      if (generation !== this.generation || !this.node) return;
      if (Date.now() >= this.nextRenew) {
        await this.api(`/v1/node-sessions/${this.node.node_session_id}/renew`, "POST");
        this.nextRenew = Date.now() + 20_000;
      }
      if (this.node.control_required && instance && Date.now() >= this.nextControl) {
        await this.maintainControl(generation).catch((error: unknown) => {
          if (error instanceof AccountError && error.status === 401) throw error;
          this.nextControl = Date.now() + retryDelay(this.controlFailures++);
          this.update({
            hostOnline: false,
            error: error instanceof Error ? error.message : "Failed to bring this host online.",
          });
        });
      }
      if (generation !== this.generation) return;
      if (Date.now() >= this.nextDiscovery) {
        try {
          const list = await this.api<{ hosts: AccountHost[] }>(
            `/v1/hosts/online?exclude_node_id=${this.node.node_id}`,
          );
          if (generation !== this.generation) return;
          this.discoveryFailures = 0;
          this.nextDiscovery = Date.now() + 10_000;
          this.update({
            status: "online",
            hosts: list.hosts.filter((h) => h.host_id !== this.node?.host_id),
            stale: false,
            error: null,
          });
        } catch (error) {
          this.nextDiscovery =
            Date.now() +
            Math.min(60_000, 10_000 * 2 ** Math.min(this.discoveryFailures++, 3)) *
              (0.8 + Math.random() * 0.4);
          this.update({ stale: true });
          throw error;
        }
      }
    } catch (error) {
      if (generation !== this.generation) return;
      if (
        error instanceof AccountError &&
        ["node_session_expired", "registration_expired"].includes(error.code)
      ) {
        this.deps.closeTransports();
        await this.deps.local("DELETE").catch(() => undefined);
        this.node = null;
        this.runtimeInstance = null;
        this.registrationId = this.deps.randomUUID();
      } else if (error instanceof AccountError && error.status === 401) {
        // Do not await logout from the tick it must drain.
        queueMicrotask(() => {
          if (generation !== this.generation) return;
          void this.logout().then(() => {
            if (!this.account) this.update({ error: error.message });
          });
        });
      }
      if (!this.node) this.nextRegistration = Date.now() + retryDelay(this.registrationFailures++);
      this.update({
        status: "error",
        error: error instanceof Error ? error.message : "Failed to connect to the account service.",
        stale: true,
      });
    }
  }

  private async maintainControl(generation: number): Promise<void> {
    const status = (await this.deps.local("GET")) as { online: boolean; connecting: boolean };
    if (generation !== this.generation || !this.node || !this.account) return;
    this.update({ hostOnline: status.online });
    if (status.online) {
      this.controlFailures = 0;
      return;
    }
    if (status.connecting) return;
    const grant = await this.api<{ control_ticket: string }>(
      `/v1/node-sessions/${this.node.node_session_id}/control-tickets`,
      "POST",
    );
    if (generation !== this.generation || !this.node || !this.account) return;
    await this.deps.local("PUT", {
      center_url: this.account.center,
      control_ticket: grant.control_ticket,
      node_session_id: this.node.node_session_id,
    });
    this.nextControl = Date.now() + retryDelay(this.controlFailures++);
  }

  private api<T = unknown>(path: string, method = "GET", body?: unknown): Promise<T> {
    if (!this.account) return Promise.reject(new Error("Sign in first."));
    return this.http(this.account.center, this.account.token, path, method, body);
  }

  private async http<T>(
    center: string,
    token: string | null,
    path: string,
    method: string,
    body?: unknown,
    requests = this.requests,
  ): Promise<T> {
    const abort = new AbortController();
    requests.add(abort);
    const timer = setTimeout(() => abort.abort(), 5000);
    try {
      const response = await (this.deps.fetch ?? fetch)(`${center}${path}`, {
        method,
        redirect: "error",
        signal: abort.signal,
        headers: {
          "Content-Type": "application/json",
          ...(token ? { Authorization: `Bearer ${token}` } : {}),
        },
        ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      });
      if (response.status === 204) return undefined as T;
      // Gateways can answer with HTML or an empty body; keep the status for callers.
      const value = (await response.json().catch(() => ({}))) as {
        error?: { message?: string; code?: string };
      };
      if (!response.ok)
        throw new AccountError(
          value.error?.message ?? "Account service request failed.",
          response.status,
          value.error?.code ?? "request_failed",
        );
      return value as T;
    } finally {
      clearTimeout(timer);
      requests.delete(abort);
    }
  }
}

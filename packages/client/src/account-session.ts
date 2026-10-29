import { DeviceAuthorizationSchema, type DeviceAuthorization } from "@ait/protocol/publication";
import { AccountChannel, type ChannelHost } from "./account-channel.js";
import type { DaemonTransportFactory } from "./daemon-client-transport-types.js";
import { HUB_PATH, HUB_PROTOCOL } from "./hub/frame.js";
import type { HubLink, HubTicket } from "./hub/link.js";
import {
  createWebSocketTransportFactory,
  defaultWebSocketFactory,
} from "./daemon-client-websocket-transport.js";
import {
  accountLoginError,
  parseAccountCallback,
  type AccountBrowserLogin,
} from "./account-browser-login.js";

/** API base includes the management gateway prefix; all relay URLs derive from it. */
export const DEFAULT_ACCOUNT_CENTER = "https://aitapp.cn/api";

export interface AccountHost extends ChannelHost {
  /** Current access-link presence; independent of the legacy Relay route. */
  hub_online?: boolean;
}

interface LoginUser {
  display_name?: string | null;
  email?: string | null;
  phone_number?: string | null;
  expires_at?: string | null;
}

/** Account details safe to display outside the platform credential owner. */
export interface AccountInfo {
  loginName: string;
  expiresAt: string | null;
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
  /** Hosts individually confirmed by online events on the current channel while the list is stale. */
  confirmedHostIds?: string[];
  error: string | null;
  selected: AccountHost | null;
  /** Compatibility snapshot field; daemon publication is now independent of the account client. */
  synchronizedHosts?: string[];
  accountExpiresAt?: string | null;
  loginPending?: boolean;
  /** Stable subject of the signed-in account; scopes hosts persisted from this account. */
  accountId?: string;
}

export interface AccountDependencies {
  installationId: string;
  deviceName: string;
  platform: string;
  randomUUID(): string;
  appVersion: string;
  save(account: SavedAccount | null): Promise<void>;
  notify(snapshot: AccountSnapshot): void;
  closeTransports(): void;
  fetch?: typeof fetch;
  channelTransport?: DaemonTransportFactory;
  /** Browsers bind one-use tickets to their Origin via the WebSocket subprotocol. */
  channelAuthentication?: "bearer" | "subprotocol";
  browserLogin?: AccountBrowserLogin;
  /** Opens a service page in the browser; services may require it to confirm account deletion. */
  openUrl?(url: string): Promise<unknown>;
}

export interface AccountRuntime {
  serverId: string;
  instanceId: string;
  name: string;
  platform: string;
}

/** Current Online Service access is authoritative on the account server. */
export interface OnlineServiceMembership {
  required: boolean;
  purchases_available: boolean;
  app_account_token: string;
  product_id: string;
  can_connect: boolean;
  status: string;
  expires_at: string | null;
}

export interface AppStoreTransaction {
  transaction_id: string;
  environment: "Production" | "Sandbox";
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
  const center = url.toString().replace(/\/+$/, "");
  // Migrate former official endpoints, including saved accounts and relay hosts.
  return center === "https://dash.ait-app.com:8443/api" || center === "https://aitapp.cn:8443/api"
    ? DEFAULT_ACCOUNT_CENTER
    : center;
}

/** A relay visit started during startup may wait this long for this device's registration. */
const NODE_READY_WAIT_MS = 10_000;
function hubTicket(grant: { hub_ticket?: unknown; hub_path?: unknown; protocol?: unknown }) {
  if (
    grant.protocol !== HUB_PROTOCOL ||
    grant.hub_path !== HUB_PATH ||
    typeof grant.hub_ticket !== "string" ||
    !/^[0-9a-f]{64}$/i.test(grant.hub_ticket)
  )
    throw new Error("The service returned an invalid hub ticket.");
  return grant.hub_ticket;
}

/** Read the unverified `sub` claim; the service validates the token on every request. */
export function accountSubject(token: string): string | undefined {
  try {
    const payload = token.split(".")[1];
    if (!payload) return undefined;
    const padded = payload.replace(/-/g, "+").replace(/_/g, "/");
    const sub = JSON.parse(atob(padded.padEnd(Math.ceil(padded.length / 4) * 4, "="))).sub;
    return typeof sub === "string" && sub ? sub : undefined;
  } catch {
    return undefined;
  }
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
  private generation = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private running: Promise<void> | null = null;
  private tickRequested = false;
  private requests = new Set<AbortController>();
  private nodeWaiters = new Set<() => void>();
  private nextRenew = 0;
  private channel: AccountChannel | null = null;
  private confirmedHosts = new Set<string>();
  private nextChannel = 0;
  private channelFailures = 0;
  private nextRegistration = 0;
  private registrationFailures = 0;
  private leaseRecoveryAttempted = false;
  private closing = false;
  private hub: HubLink | null = null;
  private unsubscribeHub: (() => void) | null = null;
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

  /** Share access presence with the existing account/Host list on every platform. */
  attachHub(hub: HubLink): void {
    this.unsubscribeHub?.();
    this.hub = hub;
    this.unsubscribeHub = hub.subscribe(() => this.update({}));
  }

  snapshot(): AccountSnapshot {
    const hub = this.hub;
    const ready = Boolean(
      this.account && !this.suspended && hub?.ready && hub.centerUrl === this.state.center,
    );
    const confirmed = new Set(this.confirmedHosts);
    const cloneHost = (host: AccountHost): AccountHost => ({
      ...host,
      relay_modes: [...host.relay_modes],
      ...(ready && hub?.serveFor(host.host_id) ? { online: true, hub_online: true } : {}),
    });
    const hosts = this.state.hosts.map(cloneHost);
    for (const host of hosts) if (host.hub_online) confirmed.add(host.host_id);
    return {
      ...this.state,
      ...(ready ? { status: "online" as const, error: null } : {}),
      confirmedHostIds: [...confirmed],
      hosts,
      selected: this.state.selected ? cloneHost(this.state.selected) : null,
      synchronizedHosts: [],
      ...(this.account ? { accountId: accountSubject(this.account.token) } : {}),
    };
  }

  private update(patch: Partial<AccountSnapshot>): void {
    this.state = { ...this.state, ...patch };
    // Registration, sign-out and suspension all publish a snapshot; waiters re-check then.
    if (this.node || !this.account || this.suspended) {
      for (const release of this.nodeWaiters) release();
    }
    this.deps.notify(this.snapshot());
  }

  /** Resolve once this device is registered, signed out or suspended, or after the timeout. */
  private waitForNode(timeoutMs: number): Promise<void> {
    return new Promise((resolve) => {
      const release = () => {
        clearTimeout(timer);
        this.nodeWaiters.delete(release);
        resolve();
      };
      const timer = setTimeout(release, timeoutMs);
      this.nodeWaiters.add(release);
    });
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

  /**
   * Resolves `false` when the service requires confirmation in the browser. The session then
   * stays signed in; the service revokes it once the user confirms there.
   */
  async deleteAccount(): Promise<boolean> {
    const account = this.account;
    const generation = this.generation;
    if (!account || this.suspended || this.closing) throw new Error("Sign in first.");
    const changed = () => this.account !== account || this.generation !== generation;
    try {
      await this.api("/v1/me", "DELETE");
    } catch (error) {
      // Accounts linked to the identity provider are deleted only after fresh browser sign-in.
      if (
        !(error instanceof AccountError && error.code === "browser_confirmation_required") ||
        !this.deps.openUrl
      )
        throw error;
      if (changed()) throw new Error("The account changed. Refresh and try again.");
      await this.deps.openUrl(`${account.center}/v1/auth/delete/authorize`);
      return false;
    }
    if (changed()) throw new Error("The account changed. Refresh and try again.");
    // The center revokes this account’s machine grants; this client only clears its own session.
    await this.logout();
    return true;
  }

  private async disconnectAccount(forgetAccount: boolean): Promise<void> {
    this.cancelLogin();
    this.generation += 1;
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    for (const request of this.requests) request.abort();
    this.closeChannel();
    this.deps.closeTransports();
    const account = this.account;
    const node = this.node;
    this.account = null;
    this.node = null;
    this.registrationId = this.deps.randomUUID();
    this.nextChannel = this.nextRenew = this.nextRegistration = 0;
    this.channelFailures = this.registrationFailures = 0;
    this.leaseRecoveryAttempted = false;
    await this.running?.catch(() => undefined);
    if (account && node) {
      await this.http(
        account.center,
        account.token,
        `/v1/node-sessions/${node.node_session_id}`,
        "DELETE",
      ).catch(() => undefined);
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
  }

  async select(hostId: string | null): Promise<AccountSnapshot> {
    if (hostId === null) {
      this.update({ selected: null });
      return this.snapshot();
    }
    if (!this.account || !this.node)
      throw new Error("Sign in and wait for this device to register.");
    const host = this.state.hosts.find((host) => host.host_id === hostId);
    if (!host || !host.online || !host.server_id || !host.instance_id)
      throw new Error("The target host is offline. Refresh the host list.");
    if (this.state.selected?.host_id !== hostId) {
      this.update({ selected: host });
    }
    return this.snapshot();
  }

  refresh(): void {
    this.channel?.refresh({ verify: true });
    this.nextChannel = 0;
    this.nextRegistration = 0;
    this.nextRenew = 0;
    this.schedule(0);
  }

  async getAccountInfo(): Promise<AccountInfo> {
    const account = this.account;
    const generation = this.generation;
    if (!account || this.suspended || this.closing) throw new Error("Sign in first.");
    const user = await this.api<LoginUser>("/v1/me");
    if (this.account !== account || this.generation !== generation)
      throw new Error("The account changed. Refresh and try again.");
    const loginName = [user.email, user.phone_number, user.display_name].find(
      (value): value is string => typeof value === "string" && value.trim().length > 0,
    );
    if (
      !loginName ||
      (user.expires_at !== null && !Number.isFinite(Date.parse(user.expires_at ?? "")))
    )
      throw new Error("The service returned invalid account information.");
    return {
      loginName: loginName.trim(),
      expiresAt: user.expires_at ?? null,
    };
  }

  getMembership(): Promise<OnlineServiceMembership> {
    return this.membershipRequest();
  }

  verifyAppStoreTransaction(transaction: AppStoreTransaction): Promise<OnlineServiceMembership> {
    return this.membershipRequest(transaction);
  }

  private async membershipRequest(
    transaction?: AppStoreTransaction,
  ): Promise<OnlineServiceMembership> {
    const account = this.account;
    const generation = this.generation;
    if (!account || this.suspended || this.closing) throw new Error("Sign in first.");
    const value = await this.http<OnlineServiceMembership>(
      account.center,
      account.token,
      transaction ? "/v1/billing/app-store/transactions" : "/v1/billing/membership",
      transaction ? "POST" : "GET",
      transaction,
      this.requests,
      25_000,
    );
    if (this.account !== account || this.generation !== generation)
      throw new Error("The account changed. Refresh and try again.");
    if (
      typeof value.required !== "boolean" ||
      typeof value.purchases_available !== "boolean" ||
      typeof value.can_connect !== "boolean" ||
      typeof value.status !== "string" ||
      typeof value.product_id !== "string" ||
      typeof value.app_account_token !== "string" ||
      !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(
        value.app_account_token,
      ) ||
      (value.expires_at !== null &&
        (typeof value.expires_at !== "string" || !Number.isFinite(Date.parse(value.expires_at))))
    )
      throw new Error("The service returned invalid membership information.");
    return value;
  }

  /** Approve one machine authorization. The daemon consumes and maintains its own credentials. */
  async authorizeHost(runtime: AccountRuntime): Promise<DeviceAuthorization> {
    for (const value of [runtime.serverId, runtime.instanceId, runtime.name, runtime.platform]) {
      if (typeof value !== "string" || !value.trim() || value.length > 320)
        throw new Error("Invalid host identity.");
    }
    const account = this.account;
    const generation = this.generation;
    if (!account || this.suspended || this.closing) throw new Error("Sign in first.");
    if (account.center !== DEFAULT_ACCOUNT_CENTER)
      throw new Error(
        "This daemon uses the official account service. Sign in there to authorize it.",
      );
    const current = () => {
      if (this.account !== account || this.generation !== generation)
        throw new Error("The account changed. Try again.");
    };
    const started = Date.now();
    const codes = await this.http<{
      device_code: string;
      user_code: string;
      expires_in: number;
      interval: number;
    }>(account.center, null, "/v1/auth/device/authorize", "POST", {
      client_id: "ait-daemon",
      machine: {
        server_id: runtime.serverId,
        display_name: runtime.name,
        platform: runtime.platform,
        app_version: this.deps.appVersion,
      },
    });
    current();
    if (!Number.isInteger(codes.expires_in) || codes.expires_in < 1 || codes.expires_in > 600)
      throw new Error("The service returned an invalid machine authorization.");
    const authorization = DeviceAuthorizationSchema.parse({
      kind: "device_code",
      device_code: codes.device_code,
      user_code: codes.user_code,
      request_id: this.deps.randomUUID(),
      expires_at: new Date(started + codes.expires_in * 1000).toISOString(),
      interval: codes.interval,
    });
    await this.api("/v1/auth/device/approve", "POST", { user_code: codes.user_code });
    current();
    return authorization;
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
    // Cold starts restore saved hosts before this device has registered. Let their
    // visits wait for the registration in flight instead of failing into backoff.
    if (!this.suspended && this.account && !this.node) await this.waitForNode(NODE_READY_WAIT_MS);
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

  /** One-use access ticket for this device's hub link; waits briefly for registration. */
  async openHubTicket(): Promise<HubTicket> {
    if (!this.suspended && this.account && !this.node) await this.waitForNode(NODE_READY_WAIT_MS);
    if (this.suspended || !this.account || !this.node) {
      throw new Error("Sign in and wait for this device to register.");
    }
    const generation = this.generation;
    const center = this.account.center;
    const grant = await this.api<{ hub_ticket: string; hub_path: string; protocol: string }>(
      `/v1/node-sessions/${this.node.node_session_id}/hub-tickets`,
      "POST",
      { ep: "access" },
    );
    if (generation !== this.generation) throw new Error("Connection cancelled.");
    return { centerUrl: center, ticket: hubTicket(grant) };
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
    this.closeChannel();
    this.deps.closeTransports();
    this.update({ stale: true });
    this.schedule(0);
  }

  /** Revalidate the node lease and discover hosts when the app becomes active. */
  resume(): void {
    this.suspended = false;
    this.nextRenew = 0;
    this.refresh();
  }

  private schedule(delay: number): void {
    if (this.closing || !this.account || this.suspended) return;
    if (this.running) {
      if (delay === 0) this.tickRequested = true;
      return;
    }
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = null;
      this.running = this.tick().finally(() => {
        this.running = null;
        const delay = this.tickRequested ? 0 : 2000;
        this.tickRequested = false;
        this.schedule(delay);
      });
    }, delay);
    this.timer.unref?.();
  }

  private async tick(): Promise<void> {
    if (!this.account || this.suspended || this.closing) return;
    const generation = this.generation;
    try {
      if (this.account.expiresAt <= Date.now())
        throw new AccountError("Your session has expired. Sign in again.", 401, "unauthorized");
      if (!this.node) {
        if (Date.now() < this.nextRegistration) return;
        const node = await this.api<NodeSession>("/v1/nodes/register", "POST", {
          registration_id: this.registrationId,
          installation_id: this.deps.installationId,
          display_name: this.deps.deviceName,
          platform: this.deps.platform,
          app_version: this.deps.appVersion,
          runtime: null,
        });
        if (generation !== this.generation || !this.account) return;
        this.closeChannel();
        this.node = node;
        this.registrationFailures = 0;
        this.nextRegistration = 0;
        this.account.nodeSessionId = node.node_session_id;
        await this.deps.save(this.account);
        this.nextRenew = Date.now() + 20_000;
        this.nextChannel = 0;
        this.update({ status: "online", error: null });
      }
      if (generation !== this.generation || !this.node) return;
      if (Date.now() >= this.nextRenew) {
        await this.api(`/v1/node-sessions/${this.node.node_session_id}/renew`, "POST");
        if (generation !== this.generation) return;
        this.leaseRecoveryAttempted = false;
        this.nextRenew = Date.now() + 20_000;
      }
      if (generation !== this.generation) return;
      if (!this.channel && Date.now() >= this.nextChannel) {
        try {
          await this.connectChannel(generation);
        } catch (error) {
          this.nextChannel = Date.now() + retryDelay(this.channelFailures++);
          throw error;
        }
      }
    } catch (error) {
      if (generation !== this.generation) return;
      let registerImmediately = false;
      if (
        error instanceof AccountError &&
        ["node_session_expired", "registration_expired"].includes(error.code)
      ) {
        // An expired existing lease is expected after suspension. Register once
        // immediately; a failed registration still uses the normal backoff.
        registerImmediately = this.node !== null && !this.leaseRecoveryAttempted;
        this.leaseRecoveryAttempted = true;
        this.closeChannel();
        this.deps.closeTransports();
        if (generation !== this.generation) return;
        this.node = null;
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
      if (registerImmediately) {
        this.nextRegistration = 0;
        this.registrationFailures = 0;
        this.schedule(0);
      } else if (!this.node) {
        this.nextRegistration = Date.now() + retryDelay(this.registrationFailures++);
      }
      this.confirmedHosts.clear();
      this.update({
        status: "error",
        error: error instanceof Error ? error.message : "Failed to connect to the account service.",
        stale: true,
      });
    }
  }

  private closeChannel(): void {
    this.confirmedHosts.clear();
    const channel = this.channel;
    this.channel = null;
    channel?.close();
  }

  private async connectChannel(generation: number): Promise<void> {
    if (!this.account || !this.node) return;
    const account = this.account;
    const node = this.node;
    const grant = await this.api<{
      channel_ticket: string;
      channel_path: string;
      protocol: string;
    }>("/v1/relay/channel-tickets", "POST", { source_node_session_id: node.node_session_id });
    if (generation !== this.generation || this.node !== node || this.suspended || this.closing)
      return;
    if (
      grant.protocol !== "ait-channel-v1" ||
      grant.channel_path !== "/v1/relay/channel" ||
      typeof grant.channel_ticket !== "string" ||
      !/^[A-Za-z0-9_-]{1,512}$/.test(grant.channel_ticket)
    )
      throw new Error("The service returned an invalid account channel ticket.");
    const url = new URL(`${account.center}${grant.channel_path}`);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    const create =
      this.deps.channelTransport ?? createWebSocketTransportFactory(defaultWebSocketFactory);
    const transport = create({
      url: url.toString(),
      ...(this.deps.channelAuthentication === "bearer"
        ? { headers: { Authorization: `Bearer ${grant.channel_ticket}` } }
        : { protocols: [`ait.ticket.${grant.channel_ticket}`] }),
    });
    const channel = new AccountChannel(transport, {
      snapshot: (hosts) => {
        if (this.channel !== channel) return;
        this.leaseRecoveryAttempted = false;
        this.channelFailures = 0;
        this.confirmedHosts.clear();
        const visible = hosts.filter(
          (host) => host.host_id !== node.host_id && host.node_id !== node.node_id,
        );
        this.update({
          status: "online",
          hosts: visible,
          selected: visible.find((host) => host.host_id === this.state.selected?.host_id) ?? null,
          stale: false,
          error: null,
        });
      },
      online: (host) => {
        if (
          this.channel !== channel ||
          host.host_id === node.host_id ||
          host.node_id === node.node_id
        )
          return;
        this.confirmedHosts.add(host.host_id);
        const hosts = this.state.hosts.filter((entry) => entry.host_id !== host.host_id);
        hosts.push(host);
        hosts.sort((left, right) => left.host_id.localeCompare(right.host_id));
        this.update({
          hosts,
          selected: hosts.find((entry) => entry.host_id === this.state.selected?.host_id) ?? null,
        });
      },
      offline: (hostId, observedAt) => {
        if (this.channel !== channel) return;
        this.confirmedHosts.delete(hostId);
        const hosts = this.state.hosts.map((host) =>
          host.host_id === hostId
            ? {
                ...host,
                online: false,
                instance_id: null,
                relay_modes: [],
                offline_since: host.offline_since ?? observedAt,
              }
            : host,
        );
        this.update({
          hosts,
          selected: hosts.find((host) => host.host_id === this.state.selected?.host_id) ?? null,
        });
      },
      failed: (recoverImmediately) => {
        if (this.channel !== channel) return;
        this.closeChannel();
        this.nextChannel = recoverImmediately ? 0 : Date.now() + retryDelay(this.channelFailures++);
        this.nextRenew = 0;
        this.update({
          status: "error",
          stale: true,
          error: "Account status connection lost. Reconnecting.",
        });
        this.schedule(0);
      },
    });
    this.channel = channel;
    this.confirmedHosts.clear();
    this.update({ status: "connecting", stale: true });
    channel.start();
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
    timeoutMs = 5000,
  ): Promise<T> {
    const abort = new AbortController();
    requests.add(abort);
    const timer = setTimeout(() => abort.abort(), timeoutMs);
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

import { DeviceAuthorizationSchema, type DeviceAuthorization } from "@ait/protocol/publication";
import {
  HUB_CENTER_FEATURES,
  HUB_PATH,
  HUB_PROTOCOL,
  type HubHost,
  type HubLane,
} from "./hub/frame.js";
import type { HubLink, HubTicket } from "./hub/link.js";
import type { HubSession } from "./hub/session.js";
import {
  accountLoginError,
  parseAccountCallback,
  type AccountBrowserLogin,
} from "./account-browser-login.js";

/** API base includes the management gateway prefix; all relay URLs derive from it. */
export const DEFAULT_ACCOUNT_CENTER = "https://aitapp.cn/api";

/** One Host of the account, built from the Hub link's `hosts` frames. */
export interface AccountHost {
  host_id: string;
  node_id: string | null;
  server_id: string | null;
  instance_id: string | null;
  name: string;
  hostname?: string | null;
  platform: string | null;
  app_version?: string | null;
  relay_modes: string[];
  online: boolean;
  status: string;
  last_seen_at?: string | null;
  offline_since?: string | null;
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

/** Hub fields of a registration. */
interface RegistrationHub {
  hub_features?: unknown;
  hub_ticket?: unknown;
  hub_path?: unknown;
  hub_expires_in_seconds?: unknown;
  hub_ep?: unknown;
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
  /** Hosts the Hub link currently reports online; empty while the list is stale. */
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
/** Use a registration's hub ticket only while comfortably inside its lifetime. */
const TICKET_MARGIN_MS = 5000;
/** A bulk link without download sessions for this long is closed. */
const BULK_IDLE_MS = 60_000;
/** A download waits this long for a new bulk link before opening on the main link. */
const BULK_CONNECT_WAIT_MS = 5000;
/** After a bulk link fails to connect, downloads use the main link for this long. */
const BULK_RETRY_MS = 60_000;

/**
 * Require every feature of `HUB_CENTER_FEATURES` in an HTTP response's `hub_features`. This client
 * works only with centers that advertise them; the center stays compatible with older clients.
 */
function requireHubFeatures(value: unknown): void {
  const advertised = Array.isArray(value) ? value : [];
  const missing = HUB_CENTER_FEATURES.filter((feature) => !advertised.includes(feature));
  if (missing.length)
    throw new Error(
      `The account service is too old for this app (missing Hub features: ${missing.join(", ")}). Update the service.`,
    );
}

/** Map a `hosts` frame entry onto the Host shape the apps consume. */
function accountHost(host: HubHost): AccountHost {
  const online = host.state !== "offline";
  return {
    host_id: host.host_id,
    node_id: host.node_id,
    server_id: host.server_id,
    instance_id: online ? host.instance_id : null,
    name: host.name,
    hostname: host.hostname,
    platform: host.platform,
    app_version: host.app_version,
    relay_modes: [],
    online,
    status: "active",
    last_seen_at: null,
    offline_since: online ? null : host.offline_since,
    hub_online: online,
  };
}

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
  private nextRegistration = 0;
  private registrationFailures = 0;
  private leaseRecoveryAttempted = false;
  private closing = false;
  private hub: HubLink | null = null;
  private unsubscribeHub: (() => void) | null = null;
  /** Download-only link (`lane: "bulk"`), created by the first download. */
  private bulk: HubLink | null = null;
  private unsubscribeBulk: (() => void) | null = null;
  private bulkRetryAt = 0;
  /** One-use access ticket issued with the registration, for the link's first connection. */
  private registeredTicket: { ticket: string; expiresAt: number; node: NodeSession } | null = null;
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

  /** The account's Host list and this device's node lease come from the Hub link. */
  attachHub(hub: HubLink): void {
    this.unsubscribeHub?.();
    this.closeBulk();
    this.hub = hub;
    this.unsubscribeHub = hub.subscribe(() => {
      // The bulk link lives only as long as the main link.
      if (hub.state === "stopped") this.closeBulk();
      // A welcomed link holds the lease: the node session is evidently valid again.
      if (this.leaseByHub()) this.leaseRecoveryAttempted = false;
      this.update({});
    });
  }

  /**
   * The link to this account's center exists (online or within its grace period), so the center
   * renews this device's node lease (`lease`). Without one, e.g. while the link cannot connect,
   * HTTP renewal keeps the node session (and Relay downloads from older Hosts) alive.
   */
  private leaseByHub(): boolean {
    return Boolean(this.account && this.hub?.centerUrl === this.account.center);
  }

  /** Host list built from `hosts` frames; stale until the link has a full snapshot. */
  snapshot(): AccountSnapshot {
    const hub = this.hub;
    const ready = Boolean(
      this.account && !this.suspended && hub?.ready && hub.centerUrl === this.state.center,
    );
    const node = this.node;
    const current = this.account && hub?.centerUrl === this.account.center;
    const hosts = (current ? (hub.hosts ?? []) : [])
      .filter((host) => host.host_id !== node?.host_id && host.node_id !== node?.node_id)
      .map(accountHost)
      .sort((left, right) => left.host_id.localeCompare(right.host_id));
    const selected = hosts.find((host) => host.host_id === this.state.selected?.host_id) ?? null;
    return {
      ...this.state,
      ...(ready ? { status: "online" as const, error: null } : {}),
      stale: !ready,
      confirmedHostIds: ready
        ? hosts.filter((host) => host.online).map((host) => host.host_id)
        : [],
      hosts,
      selected,
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
    this.closeBulk();
    this.bulkRetryAt = 0;
    this.deps.closeTransports();
    const account = this.account;
    const node = this.node;
    this.account = null;
    this.node = null;
    this.registeredTicket = null;
    this.registrationId = this.deps.randomUUID();
    this.nextRenew = this.nextRegistration = 0;
    this.registrationFailures = 0;
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
    const host = this.snapshot().hosts.find((host) => host.host_id === hostId);
    if (!host || !host.online || !host.server_id || !host.instance_id)
      throw new Error("The target host is offline. Refresh the host list.");
    if (this.state.selected?.host_id !== hostId) {
      this.update({ selected: host });
    }
    return this.snapshot();
  }

  refresh(): void {
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

  async openDownload(
    hostId: string,
    token: string,
    expectedCenter: string,
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
    }>(`/v1/hosts/${hostId}/relay-downloads`, "POST", {
      source_node_session_id: this.node.node_session_id,
      attempt_id: this.deps.randomUUID(),
      mode: "ait-download-v1",
      download_token: token,
    });
    if (generation !== this.generation) {
      await this.closeVisit(result.relay_session_id);
      throw new Error("Connection cancelled.");
    }
    const url = new URL(`${center}/v1/relay/sessions/${result.relay_session_id}/client`);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    return { ...result, url: url.toString() };
  }

  /**
   * Open a download as a Hub session (`k:"download"`) when the target Host enabled `download`.
   * Resolves null otherwise (an older Host): the caller uses `openDownload`, the legacy
   * `/relay-downloads` route. The session carries the relay download messages unchanged. It runs
   * on a separate bulk link, so download data never queues ahead of RPC responses; while no bulk
   * link can connect it opens on the main link.
   */
  async openHubDownload(
    hostId: string,
    token: string,
    expectedCenter: string,
  ): Promise<HubSession | null> {
    const hub = this.hub;
    if (!hub || this.suspended || !this.account) return null;
    if (normalizeCenter(expectedCenter) !== this.account.center) return null;
    if (hub.pending) await hub.whenReady(NODE_READY_WAIT_MS / 2);
    if (!hub.ready || hub.centerUrl !== this.account.center) return null;
    const host = hub.hostFor(hostId);
    if (!host?.node_id || host.state !== "online" || !host.f.includes("download")) return null;
    const generation = this.generation;
    const bulk = await this.bulkLink(hub);
    if (generation !== this.generation || this.hub !== hub) return null;
    if (bulk) {
      try {
        return bulk.openDownload(host.node_id, token);
      } catch {
        /* E.g. every bulk session is in use: the main link still carries downloads. */
      }
    }
    return hub.openDownload(host.node_id, token);
  }

  /**
   * The bulk link, created and connected on first use; concurrent downloads share it. Resolves
   * null when it cannot connect, and downloads use the main link for a while.
   */
  private async bulkLink(hub: HubLink): Promise<HubLink | null> {
    let link = this.bulk;
    if (!link) {
      if (Date.now() < this.bulkRetryAt) return null;
      link = hub.bulkLink(() => this.openHubTicket("bulk"), BULK_IDLE_MS);
      const created = link;
      this.bulk = created;
      // It stops itself when idle; the next download creates a new one.
      this.unsubscribeBulk = created.subscribe(() => {
        if (created.state === "stopped" && this.bulk === created) this.closeBulk();
      });
      created.start();
    }
    // A link welcomed once keeps its sessions while it resumes.
    if (!link.centerUrl) await link.whenReady(BULK_CONNECT_WAIT_MS);
    if (this.bulk !== link) return null;
    if (link.centerUrl && link.centerUrl === this.account?.center) return link;
    this.bulkRetryAt = Date.now() + BULK_RETRY_MS;
    this.closeBulk();
    return null;
  }

  /** Stop the bulk link and end its downloads. */
  private closeBulk(): void {
    const bulk = this.bulk;
    this.bulk = null;
    this.unsubscribeBulk?.();
    this.unsubscribeBulk = null;
    bulk?.stop();
  }

  /**
   * One-use access ticket for this device's hub link; waits briefly for registration. The first
   * main connection after a registration uses the ticket issued with it. Fails when the center
   * does not advertise every feature this client requires.
   */
  async openHubTicket(lane: HubLane = "main"): Promise<HubTicket> {
    if (!this.suspended && this.account && !this.node) await this.waitForNode(NODE_READY_WAIT_MS);
    if (this.suspended || !this.account || !this.node) {
      throw new Error("Sign in and wait for this device to register.");
    }
    const generation = this.generation;
    const center = this.account.center;
    const node = this.node;
    const registered = lane === "main" ? this.registeredTicket : null;
    if (lane === "main") this.registeredTicket = null;
    if (registered?.node === node && registered.expiresAt > Date.now())
      return { centerUrl: center, ticket: registered.ticket };
    let grant: { hub_ticket: string; hub_path: string; protocol: string; hub_features?: unknown };
    try {
      grant = await this.api(
        `/v1/node-sessions/${node.node_session_id}/hub-tickets`,
        "POST",
        lane === "bulk" ? { ep: "access", lane } : { ep: "access" },
      );
    } catch (error) {
      // With `lease` nothing else renews the node session; its expiry surfaces here.
      if (generation === this.generation && this.node === node) this.nodeRejected(error);
      throw error;
    }
    if (generation !== this.generation) throw new Error("Connection cancelled.");
    requireHubFeatures(grant.hub_features);
    return { centerUrl: center, ticket: hubTicket(grant) };
  }

  /** Recover from a rejected node session outside the tick: register again or sign out. */
  private nodeRejected(error: unknown): void {
    if (!(error instanceof AccountError)) return;
    if (["node_session_expired", "registration_expired"].includes(error.code)) {
      this.node = null;
      this.registeredTicket = null;
      this.registrationId = this.deps.randomUUID();
      this.nextRegistration = 0;
      this.registrationFailures = 0;
      this.update({ stale: true });
      this.schedule(0);
    } else if (error.status === 401) {
      const generation = this.generation;
      queueMicrotask(() => {
        if (generation !== this.generation) return;
        void this.logout().then(() => {
          if (!this.account) this.update({ error: error.message });
        });
      });
    }
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
    this.closeBulk();
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
        const registered = await this.api<NodeSession & RegistrationHub>(
          "/v1/nodes/register",
          "POST",
          {
            registration_id: this.registrationId,
            installation_id: this.deps.installationId,
            display_name: this.deps.deviceName,
            platform: this.deps.platform,
            app_version: this.deps.appVersion,
            runtime: null,
          },
        );
        if (generation !== this.generation || !this.account) return;
        const { hub_features, hub_ticket, hub_path, hub_expires_in_seconds, hub_ep, ...node } =
          registered;
        requireHubFeatures(hub_features);
        // The registration also issues the link's first access ticket.
        this.registeredTicket =
          hub_ep === "access" &&
          hub_path === HUB_PATH &&
          typeof hub_ticket === "string" &&
          /^[0-9a-f]{64}$/i.test(hub_ticket)
            ? {
                ticket: hub_ticket,
                expiresAt:
                  Date.now() +
                  (typeof hub_expires_in_seconds === "number" ? hub_expires_in_seconds : 30) *
                    1000 -
                  TICKET_MARGIN_MS,
                node,
              }
            : null;
        this.node = node;
        this.registrationFailures = 0;
        this.nextRegistration = 0;
        this.account.nodeSessionId = node.node_session_id;
        await this.deps.save(this.account);
        this.nextRenew = Date.now() + 20_000;
        this.update({ status: "online", error: null });
      }
      if (generation !== this.generation || !this.node) return;
      // The center renews this session while the link is attached or in grace (`lease`).
      if (Date.now() >= this.nextRenew && !this.leaseByHub()) {
        await this.api(`/v1/node-sessions/${this.node.node_session_id}/renew`, "POST");
        if (generation !== this.generation) return;
        this.leaseRecoveryAttempted = false;
        this.nextRenew = Date.now() + 20_000;
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
      this.update({
        status: "error",
        error: error instanceof Error ? error.message : "Failed to connect to the account service.",
        stale: true,
      });
    }
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

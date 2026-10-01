import { randomUUID } from "node:crypto";
import { hostname } from "node:os";

/** API base includes the management gateway prefix; all relay URLs derive from it. */
export const DEFAULT_ACCOUNT_CENTER = "https://ait.h.stdin.in:8443/api";

export interface AccountHost {
  host_id: string;
  node_id: string;
  server_id: string;
  instance_id: string;
  name: string;
  platform: string;
  relay_modes: string[];
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
}

export interface AccountDependencies {
  installationId: string;
  appVersion: string;
  runtime(): { status: string; serverId: string; instanceId?: string; features?: string[] };
  local(method: "GET" | "PUT" | "DELETE", body?: unknown): Promise<unknown>;
  save(account: SavedAccount | null): Promise<void>;
  notify(snapshot: AccountSnapshot): void;
  closeTransports(): void;
  fetch?: typeof fetch;
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
    throw new Error("中心地址需要 HTTPS；本机开发可使用 HTTP。");
  }
  return url.toString().replace(/\/+$/, "");
}

/** User authorization stays in main; runtime connectors receive short-lived tickets only. */
export class AccountSessionManager {
  private account: SavedAccount | null = null;
  private node: NodeSession | null = null;
  private registrationId = randomUUID();
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

  constructor(private readonly deps: AccountDependencies) {}

  snapshot(): AccountSnapshot {
    return structuredClone(this.state);
  }

  private update(patch: Partial<AccountSnapshot>): void {
    this.state = { ...this.state, ...patch };
    this.deps.notify(this.snapshot());
  }

  async login(center: string, username: string, password: string): Promise<AccountSnapshot> {
    center = normalizeCenter(center);
    if (!username.trim() || !password || password.length > 512)
      throw new Error("请输入用户名和密码。");
    await this.logout();
    const generation = this.generation;
    const result = await this.http<{
      access_token: string;
      expires_in: number;
      user: { display_name?: string; email: string };
    }>(center, null, "/v1/auth/login", "POST", { username, password });
    if (generation !== this.generation) throw new Error("登录已取消。");
    this.account = {
      center,
      token: result.access_token,
      expiresAt: Date.now() + result.expires_in * 1000,
      name: result.user.display_name || result.user.email,
    };
    await this.deps.save(this.account);
    this.update({ status: "connecting", center, name: this.account.name, error: null });
    this.schedule(0);
    return this.snapshot();
  }

  async restore(account: SavedAccount): Promise<void> {
    if (this.account || account.expiresAt <= Date.now()) {
      return;
    }
    this.account = { ...account, center: normalizeCenter(account.center) };
    this.update({ status: "connecting", center: this.account.center, name: account.name });
    // A previous process activation is released before registering this process.
    // If the request cannot reach the center, normal registration waits for its lease.
    if (account.nodeSessionId) {
      await this.api(`/v1/node-sessions/${account.nodeSessionId}`, "DELETE").catch(() => undefined);
    }
    this.schedule(0);
  }

  async logout(): Promise<void> {
    this.generation += 1;
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    for (const request of this.requests) request.abort();
    this.deps.closeTransports();
    const account = this.account;
    const node = this.node;
    this.account = null;
    this.node = null;
    this.registrationId = randomUUID();
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
    await this.deps.save(null);
    this.update({
      status: "logged_out",
      name: "",
      hostOnline: false,
      hosts: [],
      stale: true,
      error: null,
      selected: null,
    });
  }

  async shutdown(): Promise<void> {
    const saved = this.account ? { ...this.account } : null;
    await this.logout();
    if (saved) {
      delete saved.nodeSessionId;
      await this.deps.save(saved);
    }
  }

  async select(hostId: string | null): Promise<AccountSnapshot> {
    if (hostId === null) {
      this.deps.closeTransports();
      this.update({ selected: null });
      return this.snapshot();
    }
    if (!this.account || !this.node) throw new Error("请先登录并等待节点注册。");
    const host = this.state.hosts.find((host) => host.host_id === hostId);
    if (!host) throw new Error("目标 Host 当前不在线，请刷新列表。");
    if (this.state.selected?.host_id !== hostId) {
      this.deps.closeTransports();
      this.update({ selected: host });
    }
    return this.snapshot();
  }

  refresh(): void {
    this.nextDiscovery = 0;
    this.schedule(0);
  }

  openVisit(hostId: string) {
    return this.openSession(hostId, "ait-rust-single-v1");
  }

  openDownload(hostId: string, token: string) {
    return this.openSession(hostId, "ait-download-v1", token);
  }

  private async openSession(
    hostId: string,
    mode: string,
    token?: string,
  ): Promise<{
    relay_session_id: string;
    client_ticket: string;
    server_id: string;
    instance_id: string;
    url: string;
  }> {
    if (!this.account || !this.node || this.state.selected?.host_id !== hostId) {
      throw new Error("该 Host 未被选中，或账号已退出。");
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
        attempt_id: randomUUID(),
        mode,
        ...(token === undefined ? {} : { download_token: token }),
      },
    );
    if (generation !== this.generation || this.state.selected?.host_id !== hostId) {
      await this.closeVisit(result.relay_session_id);
      throw new Error("连接已取消。");
    }
    const url = new URL(`${center}/v1/relay/sessions/${result.relay_session_id}/client`);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    return { ...result, url: url.toString() };
  }

  async closeVisit(id: string): Promise<void> {
    if (this.account) await this.api(`/v1/relay/sessions/${id}`, "DELETE").catch(() => undefined);
  }

  private schedule(delay: number): void {
    if (!this.account) return;
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
    if (!this.account) return;
    const generation = this.generation;
    try {
      if (this.account.expiresAt <= Date.now())
        throw new AccountError("登录已过期，请重新登录。", 401, "unauthorized");
      const runtime = this.deps.runtime();
      const ready =
        runtime.status === "running" &&
        runtime.instanceId &&
        runtime.features?.includes("ait-rust-single-v1");
      const instance = ready ? runtime.instanceId! : null;
      if (this.node && this.runtimeInstance && this.runtimeInstance !== instance) {
        this.deps.closeTransports();
        await this.deps.local("DELETE").catch(() => undefined);
        await this.api(`/v1/node-sessions/${this.node.node_session_id}`, "DELETE");
        this.node = null;
        this.registrationId = randomUUID();
        this.update({ hostOnline: false });
      }
      if (!this.node || (!this.runtimeInstance && instance)) {
        if (Date.now() < this.nextRegistration) return;
        const node = await this.api<NodeSession>("/v1/nodes/register", "POST", {
          registration_id: this.registrationId,
          installation_id: this.deps.installationId,
          display_name: hostname(),
          platform: process.platform,
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
          this.nextControl =
            Date.now() +
            Math.min(30_000, 1000 * 2 ** Math.min(this.controlFailures++, 5)) *
              (0.8 + Math.random() * 0.4);
          this.update({
            hostOnline: false,
            error: error instanceof Error ? error.message : "Host 上线失败",
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
        this.registrationId = randomUUID();
      } else if (error instanceof AccountError && error.status === 401) {
        // Do not await logout from the tick it must drain.
        queueMicrotask(() => {
          void this.logout();
        });
      }
      if (!this.node)
        this.nextRegistration =
          Date.now() +
          Math.min(30_000, 1000 * 2 ** Math.min(this.registrationFailures++, 5)) *
            (0.8 + Math.random() * 0.4);
      this.update({
        status: "error",
        error: error instanceof Error ? error.message : "中心连接失败",
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
    this.nextControl =
      Date.now() +
      Math.min(30_000, 1000 * 2 ** Math.min(this.controlFailures++, 5)) *
        (0.8 + Math.random() * 0.4);
  }

  private api<T = unknown>(path: string, method = "GET", body?: unknown): Promise<T> {
    if (!this.account) return Promise.reject(new Error("请先登录。"));
    return this.http(this.account.center, this.account.token, path, method, body);
  }

  private async http<T>(
    center: string,
    token: string | null,
    path: string,
    method: string,
    body?: unknown,
  ): Promise<T> {
    const abort = new AbortController();
    this.requests.add(abort);
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
      const value = (await response.json()) as { error?: { message?: string; code?: string } };
      if (!response.ok)
        throw new AccountError(
          value.error?.message ?? "中心请求失败",
          response.status,
          value.error?.code ?? "request_failed",
        );
      return value as T;
    } finally {
      clearTimeout(timer);
      this.requests.delete(abort);
    }
  }
}

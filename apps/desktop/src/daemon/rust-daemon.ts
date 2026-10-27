import {
  DEFAULT_DESKTOP_SERVER_LISTEN,
  ServerListenSchema,
  parseServerListen,
  serverConnectAddress,
} from "@ait/protocol/server-listen";
import { spawn, type ChildProcess } from "node:child_process";
import { randomBytes } from "node:crypto";
import {
  closeSync,
  mkdirSync,
  openSync,
  readFileSync,
  readSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { createServer } from "node:net";
import { hostname, homedir } from "node:os";
import path from "node:path";
import { WebSocket } from "ws";

export function resolveDesktopDaemonHome(env: NodeJS.ProcessEnv): string {
  return path.resolve(env.AIT_SERVER_DATA_DIR || path.join(homedir(), ".ait-server-desktop"));
}

const DAEMON_LOG_ROTATE_BYTES = 10 * 1024 * 1024;

/** Keep one previous generation so a long-lived install cannot grow daemon.log without bound. */
export function rotateDaemonLog(logPath: string, limitBytes = DAEMON_LOG_ROTATE_BYTES): void {
  try {
    if ((statSync(logPath, { throwIfNoEntry: false })?.size ?? 0) > limitBytes) {
      renameSync(logPath, `${logPath}.1`);
    }
  } catch {
    /* Rotation is best effort; appending continues on the existing file. */
  }
}

const DAEMON_LOG_FILENAME = "daemon.log";
// Connection details a later Desktop session needs to reattach to a daemon kept after quit.
const RETAINED_DAEMON_FILENAME = "desktop-daemon.json";

/** Daemon that outlives the Desktop session that launched it; the token stays out of renderers. */
interface RetainedDaemon {
  pid: number;
  token: string;
  listen: string;
  configuredListen: string;
  startedAt: string;
}

function retainedDaemonPath(home: string): string {
  return path.join(home, RETAINED_DAEMON_FILENAME);
}

function readRetainedDaemon(home: string): RetainedDaemon | null {
  try {
    const value = JSON.parse(readFileSync(retainedDaemonPath(home), "utf8")) as Partial<
      Record<keyof RetainedDaemon, unknown>
    >;
    if (
      typeof value.pid === "number" &&
      Number.isInteger(value.pid) &&
      value.pid > 0 &&
      typeof value.token === "string" &&
      /^[0-9a-f]{64}$/.test(value.token) &&
      typeof value.listen === "string" &&
      typeof value.configuredListen === "string" &&
      typeof value.startedAt === "string"
    ) {
      return value as RetainedDaemon;
    }
  } catch {
    /* Missing or unreadable: there is nothing to reattach. */
  }
  return null;
}

function writeRetainedDaemon(home: string, daemon: RetainedDaemon): void {
  const file = retainedDaemonPath(home);
  const temporary = `${file}.${process.pid}.tmp`;
  writeFileSync(temporary, JSON.stringify(daemon), { mode: 0o600 });
  renameSync(temporary, file);
}

function forgetRetainedDaemon(home: string): void {
  rmSync(retainedDaemonPath(home), { force: true });
}

function isProcessAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    // ESRCH: gone. EPERM: the PID was reused by another user's process, so it is not ours.
    return false;
  }
}

/** Read new bytes appended to the daemon log since `offset`, for readiness detection. */
function readLogFrom(logPath: string, offset: number): { text: string; offset: number } {
  let descriptor: number | null = null;
  try {
    descriptor = openSync(logPath, "r");
    const size = statSync(logPath).size;
    if (size <= offset) return { text: "", offset };
    const buffer = Buffer.alloc(Math.min(size - offset, 64 * 1024));
    const bytesRead = readSync(descriptor, buffer, 0, buffer.length, offset);
    return { text: buffer.toString("utf8", 0, bytesRead), offset: offset + bytesRead };
  } catch {
    return { text: "", offset };
  } finally {
    if (descriptor !== null) closeSync(descriptor);
  }
}

export interface RustDaemonStatus {
  serverId: string;
  instanceId?: string;
  features?: string[];
  status: "starting" | "running" | "stopped" | "errored";
  listen: string | null;
  connectAddress: string | null;
  listenOverride: string | null;
  hostname: string | null;
  pid: number | null;
  home: string;
  version: string | null;
  desktopManaged: boolean;
  ownedByDesktop: boolean;
  startedAt: string | null;
  error: string | null;
}

/**
 * Owns the daemon it spawned. A daemon kept after quit is reattached only when its saved
 * record authenticates and reports the same PID; any other process using the PID is ignored.
 */
export class RustDaemonManager {
  private child: ChildProcess | null = null;
  // Reattached daemon from an earlier session; it is not a child of this process.
  private retained: RetainedDaemon | null = null;
  private token: string | null = null;
  private listenAddress: string | null = null;
  private configuredListen: string | null = null;
  private queue: Promise<unknown> = Promise.resolve();
  private state: RustDaemonStatus;

  constructor(
    private readonly options: {
      binary: string;
      home: string;
      listen?: string;
      getListen?: () => Promise<string>;
      timeoutMs?: number;
    },
  ) {
    this.state = {
      serverId: "",
      status: "stopped",
      listen: null,
      connectAddress: null,
      listenOverride: options.listen ?? null,
      hostname: null,
      pid: null,
      home: options.home,
      version: null,
      desktopManaged: true,
      ownedByDesktop: false,
      startedAt: null,
      error: null,
    };
  }

  status(): RustDaemonStatus {
    return { ...this.state };
  }

  private get logPath(): string {
    return path.join(this.options.home, DAEMON_LOG_FILENAME);
  }

  /** Restricted local relay API; the renderer never receives the local Bearer token. */
  async relayRequest(method: "GET" | "PUT" | "DELETE", body?: unknown): Promise<unknown> {
    if (this.state.status !== "running" || !this.state.connectAddress || !this.token)
      throw new Error("The local runtime is not ready.");
    if (method === "GET") {
      const info = await fetch(`http://${this.state.connectAddress}/v1/server/info`, {
        headers: { Authorization: `Bearer ${this.token}` },
        redirect: "error",
        signal: AbortSignal.timeout(5000),
      });
      if (!info.ok) throw new Error("The local runtime is temporarily unavailable.");
      const identity = (await info.json()) as {
        server_id: string;
        instance_id: string;
        features: string[];
      };
      if (identity.server_id !== this.state.serverId)
        throw new Error("The local runtime identity has changed.");
      if (identity.instance_id !== this.state.instanceId) {
        this.state = {
          ...this.state,
          instanceId: identity.instance_id,
          features: identity.features,
        };
        throw new Error("The local runtime has restarted. Registering it again.");
      }
    }
    const response = await fetch(`http://${this.state.connectAddress}/api/relay/control`, {
      method,
      redirect: "error",
      signal: AbortSignal.timeout(5000),
      headers: { Authorization: `Bearer ${this.token}`, "Content-Type": "application/json" },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    });
    if (!response.ok)
      throw new Error("The local relay API is unavailable. Check the runtime version.");
    return response.status === 204 || response.status === 202 ? null : response.json();
  }

  /** Restricted local hub serve control; rejections carry the HTTP status (404: older daemon). */
  async hubRequest(method: "GET" | "PUT" | "DELETE", body?: unknown): Promise<unknown> {
    if (this.state.status !== "running" || !this.state.connectAddress || !this.token)
      throw new Error("The local runtime is not ready.");
    const response = await fetch(`http://${this.state.connectAddress}/api/hub/control`, {
      method,
      redirect: "error",
      signal: AbortSignal.timeout(5000),
      headers: { Authorization: `Bearer ${this.token}`, "Content-Type": "application/json" },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    });
    if (!response.ok)
      throw Object.assign(new Error("The local hub API is unavailable."), {
        status: response.status,
      });
    return response.status === 204 || response.status === 202 ? null : response.json();
  }

  authorization(url: string): string | undefined {
    if (this.state.status !== "running" || !this.state.listen) return undefined;
    try {
      const requested = new URL(url);
      const owned = new URL(`ws://${this.state.connectAddress}/v1/ws`);
      return requested.protocol === "ws:" &&
        requested.port === owned.port &&
        (requested.hostname === owned.hostname ||
          (["127.0.0.1", "[::1]"].includes(owned.hostname) &&
            requested.hostname === "localhost")) &&
        requested.pathname === owned.pathname &&
        !requested.search &&
        !requested.hash &&
        !requested.username &&
        !requested.password
        ? (this.token ?? undefined)
        : undefined;
    } catch {
      return undefined;
    }
  }

  private serialize<T>(work: () => Promise<T>): Promise<T> {
    const next = this.queue.then(work);
    this.queue = next.catch(() => {});
    return next;
  }

  start(): Promise<RustDaemonStatus> {
    return this.serialize(() => this.launch());
  }
  stop(confirmed?: { pid: number; startedAt: string }): Promise<RustDaemonStatus> {
    return this.serialize(async () => {
      if (
        confirmed &&
        (confirmed.pid !== this.state.pid || confirmed.startedAt !== this.state.startedAt)
      ) {
        throw new Error(
          "Server changed since confirmation; inspect the current instance before stopping it.",
        );
      }
      await this.terminate();
      this.state = {
        ...this.state,
        status: "stopped",
        pid: null,
        listen: null,
        connectAddress: null,
        version: null,
        ownedByDesktop: false,
        error: null,
      };
      return this.status();
    });
  }

  /**
   * Leave the running daemon for the next Desktop session instead of stopping it.
   * Returns false, and changes nothing, when no healthy daemon is owned.
   */
  retainForNextSession(): Promise<boolean> {
    return this.serialize(async () => {
      const pid = this.child?.pid ?? this.retained?.pid;
      if (
        this.state.status !== "running" ||
        !pid ||
        !this.token ||
        !this.listenAddress ||
        !this.configuredListen ||
        !this.state.startedAt
      ) {
        return false;
      }
      writeRetainedDaemon(this.options.home, {
        pid,
        token: this.token,
        listen: this.listenAddress,
        configuredListen: this.configuredListen,
        startedAt: this.state.startedAt,
      });
      // Release the handle so Electron can exit without waiting for or killing the daemon;
      // this session no longer owns it, so later stops here leave it for the next session.
      this.child?.removeAllListeners();
      this.child?.unref();
      this.child = null;
      this.retained = null;
      this.token = null;
      this.state = {
        ...this.state,
        status: "stopped",
        pid: null,
        listen: null,
        connectAddress: null,
        ownedByDesktop: false,
      };
      return true;
    });
  }
  restart(): Promise<RustDaemonStatus> {
    return this.serialize(async () => {
      await this.terminate();
      return this.launch();
    });
  }

  private async terminate(): Promise<void> {
    const retained = this.retained;
    if (retained) {
      this.retained = null;
      await this.terminateRetained(retained);
      forgetRetainedDaemon(this.options.home);
    }
    const child = this.child;
    this.token = null;
    if (!child || child.exitCode !== null || child.signalCode !== null) {
      this.child = null;
      return;
    }
    await new Promise<void>((resolve) => {
      // Resolve on exit, not close: a grandchild holding stderr open must not block quit.
      let forceTimer: NodeJS.Timeout | undefined;
      const killTimer = setTimeout(() => {
        child.kill("SIGKILL");
        forceTimer = setTimeout(finish, 5_000);
      }, 15_000);
      function finish(): void {
        clearTimeout(killTimer);
        clearTimeout(forceTimer);
        resolve();
      }
      child.once("exit", finish);
      child.kill("SIGTERM");
    });
    this.child = null;
  }

  /** Stop a reattached daemon through its authenticated shutdown RPC, then by signal. */
  private async terminateRetained(retained: RetainedDaemon): Promise<void> {
    this.token = null;
    if (!isProcessAlive(retained.pid)) return;
    const address = serverConnectAddress(retained.listen);
    await requestRustDaemonShutdown(address, retained.token, 5_000).catch(() => undefined);
    if (await waitForProcessExit(retained.pid, 15_000)) return;
    // Only signal a PID that still answers with this token; a reused PID is left alone.
    const still = await probeRustDaemon(address, retained.token, 2_000).catch(() => null);
    if (still?.pid !== retained.pid) return;
    try {
      process.kill(retained.pid, "SIGKILL");
    } catch {
      /* Already exited. */
    }
    await waitForProcessExit(retained.pid, 5_000);
  }

  private async launch(): Promise<RustDaemonStatus> {
    if ((this.child || this.retained) && this.state.status === "running") return this.status();
    try {
      mkdirSync(this.options.home, { recursive: true, mode: 0o700 });
      const configuredListen = ServerListenSchema.parse(
        this.options.listen ?? (await this.options.getListen?.()) ?? DEFAULT_DESKTOP_SERVER_LISTEN,
      );
      const reattached = await this.reattach(configuredListen);
      if (reattached) return reattached;
      rotateDaemonLog(this.logPath);
      return await this.spawnDaemon(configuredListen);
    } catch (error) {
      await this.terminate();
      this.state = {
        ...this.state,
        status: "errored",
        pid: null,
        listen: null,
        connectAddress: null,
        version: null,
        ownedByDesktop: false,
        error: error instanceof Error ? error.message : String(error),
      };
      throw error;
    }
  }

  /**
   * Resume a daemon kept after an earlier quit. A record that no longer authenticates, reports
   * another PID or listens elsewhere than the current settings is shut down or discarded.
   */
  private async reattach(configuredListen: string): Promise<RustDaemonStatus | null> {
    const retained = readRetainedDaemon(this.options.home);
    if (!retained) return null;
    forgetRetainedDaemon(this.options.home);
    if (!isProcessAlive(retained.pid)) return null;
    const address = serverConnectAddress(retained.listen);
    let info: Awaited<ReturnType<typeof probeRustDaemon>>;
    try {
      info = await probeRustDaemon(address, retained.token, this.options.timeoutMs ?? 5_000);
    } catch {
      // Another process owns the PID or the daemon is unhealthy; never signal an unverified PID.
      return null;
    }
    if (info.pid !== retained.pid) return null;
    if (retained.configuredListen !== configuredListen) {
      // The listen setting changed while Ait was closed: restart under the new address.
      await this.terminateRetained(retained);
      return null;
    }
    this.retained = retained;
    this.token = retained.token;
    this.listenAddress = retained.listen;
    this.configuredListen = retained.configuredListen;
    this.state = {
      ...this.state,
      ...withoutPid(info),
      status: "running",
      pid: retained.pid,
      listen: retained.listen,
      connectAddress: address,
      startedAt: retained.startedAt,
      ownedByDesktop: true,
      hostname: hostname(),
      error: null,
    };
    return this.status();
  }

  private async spawnDaemon(configuredListen: string): Promise<RustDaemonStatus> {
    const listenAddress = await resolveListen(
      configuredListen === this.configuredListen
        ? (this.listenAddress ?? configuredListen)
        : configuredListen,
    );
    this.token = randomBytes(32).toString("hex");
    const env = Object.fromEntries(
      Object.entries(process.env).filter(([key]) => !key.startsWith("AIT_SERVER_")),
    );
    // Desktop's account lifecycle supplies the Hub connection and mobile device registrations.
    // Keep this explicit override while isolating unrelated daemon settings and credentials.
    env.AIT_SERVER_HUB_NOTIFICATIONS = process.env.AIT_SERVER_HUB_NOTIFICATIONS ?? "true";
    // The daemon writes its log file directly and runs in its own process group, so it can
    // outlive this Desktop session when the user keeps it running after quit.
    const logOffset = statSync(this.logPath, { throwIfNoEntry: false })?.size ?? 0;
    const log = openSync(this.logPath, "a", 0o600);
    let child: ChildProcess;
    try {
      child = spawn(
        this.options.binary,
        ["--data-dir", this.options.home, "--listen", listenAddress, "--log-level", "info"],
        {
          env: { ...env, AIT_SERVER_TOKEN: this.token },
          stdio: ["ignore", "ignore", log],
          detached: true,
          windowsHide: true,
        },
      );
    } finally {
      closeSync(log);
    }
    this.child = child;
    this.state = {
      ...this.state,
      status: "starting",
      pid: child.pid ?? null,
      listen: null,
      connectAddress: null,
      version: null,
      startedAt: new Date().toISOString(),
      ownedByDesktop: true,
      error: null,
    };
    let failure: Error | null = null;
    child.on("error", (error) => {
      failure = error;
    });
    child.once("exit", () => {
      if (this.child !== child) return;
      this.child = null;
      this.token = null;
      this.state = {
        ...this.state,
        status: "errored",
        pid: null,
        listen: null,
        connectAddress: null,
        version: null,
        ownedByDesktop: false,
        error: failure?.message ?? "Rust daemon exited.",
      };
    });
    const deadline = Date.now() + (this.options.timeoutMs ?? 30_000);
    let tail = "";
    let offset = logOffset;
    let listen: string | undefined;
    while (!listen && Date.now() < deadline) {
      if (failure) throw failure;
      const read = readLogFrom(this.logPath, offset);
      offset = read.offset;
      tail = (tail + read.text).slice(-16384);
      listen = tail.match(/daemon ready\s+listen=((?:\[[0-9a-fA-F:.]+\]|[0-9.]+):\d+)(?=\s)/)?.[1];
      if (listen) break;
      if (child.exitCode !== null || child.signalCode !== null || this.child !== child)
        throw new Error(tail || "Rust daemon exited before ready.");
      await new Promise((resolve) => setTimeout(resolve, 25));
    }
    if (!listen) throw new Error("Timed out waiting for Rust daemon readiness.");
    const info = await probeRustDaemon(
      serverConnectAddress(listen),
      this.token,
      Math.max(1, deadline - Date.now()),
    );
    if (this.child !== child) throw new Error("Rust daemon exited during readiness handshake.");
    this.listenAddress = listen;
    this.configuredListen = configuredListen;
    this.state = {
      ...this.state,
      ...withoutPid(info),
      listen,
      connectAddress: serverConnectAddress(listen),
      status: "running",
      hostname: hostname(),
    };
    return this.status();
  }
}

type DaemonProbe = Pick<RustDaemonStatus, "serverId" | "version" | "instanceId" | "features"> & {
  pid: number;
};

function withoutPid(info: DaemonProbe): Omit<DaemonProbe, "pid"> {
  const { pid: _pid, ...rest } = info;
  return rest;
}

const PROTOCOL = { major: 1, min_minor: 0, max_minor: 0 };

/**
 * Open an authenticated connection, negotiate `method`, send one request and resolve with the
 * handshake info and the request result. The token proves the daemon is ours before use.
 */
function rustDaemonRequest(
  listen: string,
  token: string,
  timeout: number,
  method: string,
): Promise<{ info: Record<string, unknown>; result: unknown }> {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(`ws://${listen}/v1/ws`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    const timer = setTimeout(() => finish(new Error("Rust daemon handshake timed out.")), timeout);
    let settled = false;
    let info: Record<string, unknown> | null = null;
    const finish = (error?: Error, result?: unknown) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      ws.close();
      if (error || !info) reject(error ?? new Error("Rust daemon closed before readiness."));
      else resolve({ info, result });
    };
    ws.on("error", (error) => finish(error));
    ws.on("close", () => finish(new Error("Rust daemon closed before readiness handshake.")));
    ws.on("open", () =>
      ws.send(
        JSON.stringify({
          type: "hello",
          protocol: PROTOCOL,
          client_id: "desktop-readiness",
          capabilities: [method],
          required_capabilities: [method],
        }),
      ),
    );
    ws.on("message", (data) => {
      try {
        const message = JSON.parse(data.toString());
        if (message.type === "server_info" && typeof message.info?.server_id === "string") {
          info = message.info;
          ws.send(JSON.stringify({ type: "request", request_id: "desktop", method, params: {} }));
        } else if (message.type === "response" && message.request_id === "desktop") {
          finish(undefined, message.result);
        } else if (message.type === "error") {
          finish(new Error(`Rust daemon rejected ${method}.`));
        }
      } catch {
        finish(new Error("Invalid Rust daemon readiness response."));
      }
    });
  });
}

/** Authenticated readiness check that also reports the daemon's own process ID. */
async function probeRustDaemon(
  listen: string,
  token: string,
  timeout: number,
): Promise<DaemonProbe> {
  const { info, result } = await rustDaemonRequest(
    listen,
    token,
    timeout,
    "daemon.get_status.request",
  );
  const pid = (result as { pid?: unknown } | null)?.pid;
  if (typeof pid !== "number") throw new Error("Unexpected Rust daemon readiness response.");
  return {
    serverId: info.server_id as string,
    version: typeof info.version === "string" ? info.version : null,
    instanceId: typeof info.instance_id === "string" ? info.instance_id : "",
    features: Array.isArray(info.features) ? (info.features as string[]) : [],
    pid,
  };
}

/** Ask a daemon to drain and exit through its authenticated lifecycle RPC. */
async function requestRustDaemonShutdown(
  listen: string,
  token: string,
  timeout: number,
): Promise<void> {
  await rustDaemonRequest(listen, token, timeout, "server.shutdown.request");
}

async function waitForProcessExit(pid: number, timeout: number): Promise<boolean> {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (!isProcessAlive(pid)) return true;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  return !isProcessAlive(pid);
}

// Reserve an ephemeral port before spawn, then pass a concrete address so a
// server-internal restart rebinds the same endpoint. A bind race fails closed.
async function resolveListen(listen: string): Promise<string> {
  const { host, port } = parseServerListen(listen);
  if (port !== "0") return listen;
  return new Promise((resolve, reject) => {
    const listener = createServer();
    listener.once("error", reject);
    listener.listen(0, host, () => {
      const address = listener.address();
      listener.close((error) => {
        if (error) reject(error);
        else if (!address || typeof address === "string")
          reject(new Error("Could not allocate a server port."));
        else resolve(`${host.includes(":") ? `[${host}]` : host}:${address.port}`);
      });
    });
  });
}

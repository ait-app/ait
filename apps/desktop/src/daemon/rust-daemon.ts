import {
  DEFAULT_DESKTOP_SERVER_LISTEN,
  ServerListenSchema,
  parseServerListen,
  serverConnectAddress,
} from "@ait/protocol/server-listen";
import { spawn, type ChildProcess } from "node:child_process";
import { randomBytes } from "node:crypto";
import { appendFileSync, mkdirSync, renameSync, statSync } from "node:fs";
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

/** Owns only the child it spawned; never adopts or kills a PID read from disk. */
export class RustDaemonManager {
  private child: ChildProcess | null = null;
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
  restart(): Promise<RustDaemonStatus> {
    return this.serialize(async () => {
      await this.terminate();
      return this.launch();
    });
  }

  private async terminate(): Promise<void> {
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

  private async launch(): Promise<RustDaemonStatus> {
    if (this.child && this.state.status === "running") return this.status();
    try {
      mkdirSync(this.options.home, { recursive: true, mode: 0o700 });
      const logPath = path.join(this.options.home, "daemon.log");
      rotateDaemonLog(logPath);
      const configuredListen = ServerListenSchema.parse(
        this.options.listen ?? (await this.options.getListen?.()) ?? DEFAULT_DESKTOP_SERVER_LISTEN,
      );
      const listenAddress = await resolveListen(
        configuredListen === this.configuredListen
          ? (this.listenAddress ?? configuredListen)
          : configuredListen,
      );
      this.token = randomBytes(32).toString("hex");
      const env = Object.fromEntries(
        Object.entries(process.env).filter(([key]) => !key.startsWith("AIT_SERVER_")),
      );
      const child = spawn(
        this.options.binary,
        ["--data-dir", this.options.home, "--listen", listenAddress, "--log-level", "info"],
        {
          env: { ...env, AIT_SERVER_TOKEN: this.token },
          stdio: ["ignore", "ignore", "pipe"],
          windowsHide: true,
        },
      );
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
      let tail = "";
      let failure: Error | null = null;
      child.on("error", (error) => {
        failure = error;
      });
      child.once("close", () => {
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
      child.stderr!.on("data", (chunk: Buffer) => {
        const text = chunk.toString();
        tail = (tail + text).slice(-16384);
        try {
          appendFileSync(logPath, text, { mode: 0o600 });
        } catch {
          /* A log write failure must not crash Electron or orphan its child. */
        }
      });
      const deadline = Date.now() + (this.options.timeoutMs ?? 30_000);
      let listen: string | undefined;
      while (!listen && Date.now() < deadline) {
        if (failure) throw failure;
        if (child.exitCode !== null || child.signalCode !== null || this.child !== child)
          throw new Error(tail || "Rust daemon exited before ready.");
        listen = tail.match(
          /daemon ready\s+listen=((?:\[[0-9a-fA-F:.]+\]|[0-9.]+):\d+)(?=\s)/,
        )?.[1];
        if (!listen) await new Promise((resolve) => setTimeout(resolve, 25));
      }
      if (!listen) throw new Error("Timed out waiting for Rust daemon readiness.");
      const info = await probeRustDaemon(
        serverConnectAddress(listen),
        this.token!,
        Math.max(1, deadline - Date.now()),
      );
      if (this.child !== child) throw new Error("Rust daemon exited during readiness handshake.");
      this.listenAddress = listen;
      this.configuredListen = configuredListen;
      this.state = {
        ...this.state,
        ...info,
        listen,
        connectAddress: serverConnectAddress(listen),
        status: "running",
        hostname: hostname(),
      };
      return this.status();
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
}

function probeRustDaemon(
  listen: string,
  token: string,
  timeout: number,
): Promise<Pick<RustDaemonStatus, "serverId" | "version" | "instanceId" | "features">> {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(`ws://${listen}/v1/ws`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    const timer = setTimeout(() => finish(new Error("Rust daemon handshake timed out.")), timeout);
    let settled = false;
    const finish = (
      error?: Error,
      info?: Pick<RustDaemonStatus, "serverId" | "version" | "instanceId" | "features">,
    ) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      ws.close();
      if (error) reject(error);
      else resolve(info!);
    };
    ws.on("error", (error) => finish(error));
    ws.on("close", () => finish(new Error("Rust daemon closed before readiness handshake.")));
    ws.on("open", () =>
      ws.send(
        JSON.stringify({
          type: "hello",
          protocol: { major: 1, min_minor: 0, max_minor: 0 },
          client_id: "desktop-readiness",
          capabilities: [],
          required_capabilities: [],
        }),
      ),
    );
    ws.on("message", (data) => {
      try {
        const message = JSON.parse(data.toString());
        if (message.type === "server_info" && typeof message.info?.server_id === "string")
          finish(undefined, {
            serverId: message.info.server_id,
            version: typeof message.info.version === "string" ? message.info.version : null,
            instanceId: message.info.instance_id ?? "",
            features: message.info.features ?? [],
          });
        else finish(new Error("Unexpected Rust daemon readiness response."));
      } catch {
        finish(new Error("Invalid Rust daemon readiness response."));
      }
    });
  });
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

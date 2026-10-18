import { spawn, type ChildProcess } from "node:child_process";
import { randomBytes } from "node:crypto";
import { createWriteStream } from "node:fs";
import { mkdir, mkdtemp, rm } from "node:fs/promises";
import net from "node:net";
import { tmpdir } from "node:os";
import path from "node:path";
import { registerAitDaemon, reservedDaemonPorts, unregisterAitDaemon } from "./ait-daemon";
import { killProcessTree } from "./spawn-node";

export interface IsolatedHostDaemon {
  serverId: string;
  port: number;
  token: string;
  dataDir: string;
  getPid(): number | undefined;
  restart(): Promise<void>;
  close(): Promise<void>;
}

export interface IsolatedHostDaemonOptions {
  environment?: NodeJS.ProcessEnv;
  dataDir?: string;
  preserveHome?: boolean;
}

async function getAvailablePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") {
        server.close(() => reject(new Error("Failed to acquire an isolated server port")));
        return;
      }
      server.close(() => resolve(address.port));
    });
  });
}

export async function startIsolatedHostDaemon(
  label: string,
  options: IsolatedHostDaemonOptions = {},
): Promise<IsolatedHostDaemon> {
  const binary = process.env.E2E_AIT_SERVER_BIN;
  if (!binary) throw new Error("E2E_AIT_SERVER_BIN is required; run Playwright global setup first");
  const metroPort = process.env.E2E_METRO_PORT;
  if (!metroPort) throw new Error("E2E_METRO_PORT is required to start an isolated Ait server");
  let port = await getAvailablePort();
  while (reservedDaemonPorts.has(port)) port = await getAvailablePort();
  const token = randomBytes(32).toString("hex");
  const dataDir = options.dataDir ?? (await mkdtemp(path.join(tmpdir(), "ait-e2e-server-")));
  await mkdir(dataDir, { recursive: true });
  const spawnServer = async (): Promise<{ child: ChildProcess; serverId: string }> => {
    const log = createWriteStream(path.join(dataDir, "server.log"), { flags: "a" });
    const child = spawn(
      binary,
      [
        "--data-dir",
        dataDir,
        "--listen",
        `127.0.0.1:${port}`,
        "--web-origin",
        `http://localhost:${metroPort}`,
        "--web-origin",
        `http://127.0.0.1:${metroPort}`,
      ],
      {
        cwd: dataDir,
        env: {
          ...process.env,
          // Never invoke an installed, authenticated model CLI by default.
          AIT_SERVER_CODEX_BIN: path.join(dataDir, "unconfigured-codex"),
          AIT_SERVER_CLAUDE_BIN: path.join(dataDir, "unconfigured-claude"),
          AIT_SERVER_OPENCODE_BIN: path.join(dataDir, "unconfigured-opencode"),
          AIT_SERVER_PI_BIN: path.join(dataDir, "unconfigured-pi"),
          AIT_SERVER_DEEPSEEK_HARNESS_BIN: path.join(dataDir, "unconfigured-deepseek-harness"),
          AIT_SERVER_ANTIGRAVITY_BIN: path.join(dataDir, "unconfigured-antigravity"),
          ...options.environment,
          AIT_SERVER_TOKEN: token,
          AIT_SERVER_SKILLS_HOME: path.join(dataDir, "agent-home"),
          AIT_SERVER_SKILLS_BUNDLE: path.join(dataDir, "skills-bundle"),
        },
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
    child.stdout?.pipe(log, { end: false });
    child.stderr?.pipe(log, { end: false });
    child.once("close", () => log.end());
    let stderr = "";
    let spawnError: Error | undefined;
    child.once("error", (error) => {
      spawnError = error;
    });
    child.stderr?.on("data", (chunk: Buffer) => {
      stderr = (stderr + chunk.toString("utf8")).slice(-8_000);
    });
    const deadline = Date.now() + 30_000;
    try {
      while (Date.now() < deadline) {
        if (spawnError) throw spawnError;
        if (child.exitCode !== null || child.signalCode !== null) {
          throw new Error(`Ait server ${label} exited before becoming ready`);
        }
        try {
          const response = await fetch(`http://127.0.0.1:${port}/v1/server/info`, {
            headers: { Authorization: `Bearer ${token}` },
            signal: AbortSignal.timeout(1_000),
          });
          if (!response.ok) throw new Error(`Server info returned HTTP ${response.status}`);
          const info = (await response.json()) as {
            server_id?: string;
            protocol?: { major: number };
            lifecycle?: string;
          };
          if (info.server_id && info.protocol?.major === 1 && info.lifecycle === "ready") {
            return { child, serverId: info.server_id };
          }
        } catch {
          // Retry while the owned child starts; a TCP accept alone is not readiness.
        }
        await new Promise((resolve) => setTimeout(resolve, 100));
      }
      throw new Error(`Ait server ${label} did not become ready within 30s`);
    } catch (error) {
      if (child.pid) await killProcessTree(child);
      throw new Error(`${error instanceof Error ? error.message : String(error)}\n${stderr}`, {
        cause: error,
      });
    }
  };
  let running: Awaited<ReturnType<typeof spawnServer>>;
  try {
    running = await spawnServer();
  } catch (error) {
    if (!options.preserveHome) await rm(dataDir, { recursive: true, force: true });
    throw error;
  }
  const serverId = running.serverId;
  registerAitDaemon({ port, token, serverId });
  let closed = false;
  return {
    serverId,
    port,
    token,
    dataDir,
    getPid: () => running.child.pid,
    restart: async () => {
      if (closed) throw new Error(`Cannot restart closed Ait server ${label}`);
      await killProcessTree(running.child);
      running = await spawnServer();
      if (running.serverId !== serverId) throw new Error("Ait server identity changed on restart");
    },
    close: async () => {
      if (closed) return;
      closed = true;
      unregisterAitDaemon(port);
      await killProcessTree(running.child);
      if (!options.preserveHome) await rm(dataDir, { recursive: true, force: true });
    },
  };
}

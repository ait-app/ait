import { spawn } from "node:child_process";
import { access } from "node:fs/promises";
import path from "node:path";

export const repoRoot = path.resolve(__dirname, "../../../../..");
export const reservedDaemonPorts = new Set([6767, 6768, 7316, 61680]);

export interface AitTestConnection {
  port: number;
  token: string;
  serverId: string;
}

// Only processes owned by this Playwright worker may be used by seed clients.
const connections = new Map<number, AitTestConnection>();

export function registerAitDaemon(connection: AitTestConnection): void {
  if (reservedDaemonPorts.has(connection.port)) throw new Error("Reserved server port");
  connections.set(connection.port, connection);
}

export function unregisterAitDaemon(port: number): void {
  connections.delete(port);
}

export function findAitDaemon(port: number): AitTestConnection | undefined {
  return connections.get(port);
}

export function requireAitDaemon(port: number): AitTestConnection {
  const connection = findAitDaemon(port);
  if (!connection) throw new Error(`Port ${port} is not an Ait server owned by this E2E worker`);
  return connection;
}

async function run(command: string, args: string[]): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    const child = spawn(command, args, { cwd: repoRoot, stdio: "inherit" });
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      if (code === 0) resolve();
      else reject(new Error(`${command} failed (${signal ?? code})`));
    });
  });
}

export async function prepareAitE2E(): Promise<void> {
  const npmCli = process.env.npm_execpath;
  if (npmCli) {
    await run(process.execPath, [npmCli, "run", "build:ui-deps"]);
  } else {
    await run(process.platform === "win32" ? "npm.cmd" : "npm", ["run", "build:ui-deps"]);
  }
  const configuredBinary = process.env.E2E_AITD_BIN;
  if (!configuredBinary) {
    await run("cargo", [
      "build",
      "--target-dir",
      path.join(repoRoot, "target"),
      "-p",
      "daemon",
      "--bin",
      "daemon",
    ]);
  }
  const binary = path.resolve(
    repoRoot,
    configuredBinary ??
      path.join("target", "debug", process.platform === "win32" ? "daemon.exe" : "daemon"),
  );
  await access(binary);
  process.env.E2E_AITD_BIN = binary;
}

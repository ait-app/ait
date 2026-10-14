import { randomUUID } from "node:crypto";
import { chmod, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, expect, it } from "vitest";
import { WebSocketServer } from "ws";
import { RustDaemonManager } from "./rust-daemon";
import { readServiceConnection } from "./service-connection";

const homes: string[] = [];
const servers: WebSocketServer[] = [];
const managers: RustDaemonManager[] = [];
const token = "private-service-test-token-at-least-32-characters";

async function fixture() {
  const home = await mkdtemp(path.join(tmpdir(), "ait-service-test-"));
  homes.push(home);
  const server = new WebSocketServer({ host: "127.0.0.1", port: 0 });
  servers.push(server);
  await new Promise<void>((resolve) => server.once("listening", resolve));
  const address = server.address();
  if (typeof address === "string" || !address) throw new Error("No test address");
  const record = {
    version: 1,
    managementMode: "external",
    serverId: randomUUID(),
    instanceId: randomUUID(),
    pid: process.pid,
    startedAt: new Date().toISOString(),
    listen: `127.0.0.1:${address.port}`,
    token,
  };
  const file = path.join(home, "service-connection.json");
  const save = () => writeFile(file, JSON.stringify(record), { mode: 0o600 });
  await save();
  let rejectAuthentication = false;
  server.on("connection", (ws, request) => {
    if (rejectAuthentication || request.headers.authorization !== `Bearer ${token}`) {
      ws.close();
      return;
    }
    ws.on("message", async (data) => {
      const message = JSON.parse(data.toString());
      if (message.type === "hello") {
        ws.send(
          JSON.stringify({
            type: "server_info",
            info: {
              server_id: record.serverId,
              instance_id: record.instanceId,
              version: "test",
              features: [],
            },
          }),
        );
      } else if (message.method === "server.restart.request") {
        ws.send(
          JSON.stringify({
            type: "response",
            request_id: "restart",
            result: { status: "restart_requested" },
          }),
        );
        record.instanceId = randomUUID();
        record.startedAt = new Date().toISOString();
        await save();
      }
    });
  });
  const manager = new RustDaemonManager({ binary: "/must-never-spawn", home, timeoutMs: 250 });
  managers.push(manager);
  return {
    home,
    file,
    record,
    manager,
    rejectAuthentication: () => {
      rejectAuthentication = true;
    },
  };
}

afterEach(async () => {
  for (const manager of managers.splice(0)) await manager.stop();
  for (const server of servers.splice(0)) {
    for (const socket of server.clients) socket.terminate();
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
  await Promise.all(homes.splice(0).map((home) => rm(home, { recursive: true, force: true })));
});

it("connects, detaches and reconnects without adopting the service process or consuming its record", async () => {
  const { home, file, record, manager } = await fixture();
  const before = await readFile(file, "utf8");
  const status = await manager.start();
  expect(status).toMatchObject({
    status: "running",
    managementMode: "external",
    ownedByDesktop: false,
    desktopManaged: false,
    pid: process.pid,
  });
  expect(manager.authorization(`ws://${record.listen}/v1/ws`)).toBe(token);
  expect(JSON.stringify(status)).not.toContain(token);
  await manager.stop();
  expect(await readFile(file, "utf8")).toBe(before);
  expect((await manager.start()).serverId).toBe(status.serverId);
  const reopened = new RustDaemonManager({ binary: "/must-never-spawn", home, timeoutMs: 250 });
  managers.push(reopened);
  expect((await reopened.start()).ownedByDesktop).toBe(false);
});

it("uses the authenticated restart RPC and confirms a new instance with the same PID", async () => {
  const { manager } = await fixture();
  const previous = await manager.start();
  const current = await manager.restart();
  expect(current.serverId).toBe(previous.serverId);
  expect(current.pid).toBe(previous.pid);
  expect(current.instanceId).not.toBe(previous.instanceId);
  expect(current.ownedByDesktop).toBe(false);
});

it("preserves a failed service connection and retries without launching another daemon", async () => {
  const { file, manager, rejectAuthentication } = await fixture();
  const before = await readFile(file, "utf8");
  rejectAuthentication();
  await expect(manager.start()).rejects.toThrow("external daemon is unavailable");
  await expect(manager.start()).rejects.toThrow("external daemon is unavailable");
  expect(await readFile(file, "utf8")).toBe(before);
  expect(manager.status()).toMatchObject({ managementMode: "external", ownedByDesktop: false });
  await rm(file);
  await expect(manager.start()).rejects.toThrow("external daemon is unavailable");
});

it("external mode never spawns even when discovery information is missing", async () => {
  const { home, file } = await fixture();
  await rm(file);
  const manager = new RustDaemonManager({
    binary: "/must-never-spawn",
    home,
    getMode: async () => "external",
    timeoutMs: 120,
  });
  managers.push(manager);
  await expect(manager.start()).rejects.toThrow("external daemon is unavailable");
});

it.skipIf(process.platform === "win32")(
  "rejects public credentials and symlinked records",
  async () => {
    const { home, file } = await fixture();
    await chmod(file, 0o644);
    await expect(readServiceConnection(home)).rejects.toThrow("permissions or ownership");
    await chmod(file, 0o600);
    const destination = path.join(home, "credentials.json");
    await writeFile(destination, await readFile(file), { mode: 0o600 });
    await rm(file);
    await symlink(destination, file);
    await expect(readServiceConnection(home)).rejects.toThrow("Cannot open");
  },
);

import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir, networkInterfaces } from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { RustDaemonManager, resolveDesktopDaemonHome, rotateDaemonLog } from "./rust-daemon";

const { version: expectedVersion } = JSON.parse(
  readFileSync(new URL("../../package.json", import.meta.url), "utf8"),
);
const managers: RustDaemonManager[] = [];
const homes: string[] = [];
function create(
  binary: string,
  home = mkdtempSync(path.join(tmpdir(), "ait-desktop-test-")),
  options: { getListen?: () => Promise<string>; listen?: string } = {},
) {
  homes.push(home);
  const manager = new RustDaemonManager({
    binary,
    home,
    timeoutMs: 15000,
    ...options,
  });
  managers.push(manager);
  return manager;
}
afterEach(async () => {
  for (const manager of managers.splice(0)) await manager.stop();
  for (const home of homes.splice(0)) rmSync(home, { recursive: true, force: true });
  vi.unstubAllEnvs();
});

it("reports spawn failure and allows stop after a failed start", async () => {
  const manager = create("/missing/ait-server");
  await expect(manager.start()).rejects.toThrow();
  expect(manager.status()).toMatchObject({ status: "errored", ownedByDesktop: false });
  await expect(manager.stop()).resolves.toMatchObject({ status: "stopped" });
});

it("rotates an oversized daemon log and leaves small logs in place", () => {
  const home = mkdtempSync(path.join(tmpdir(), "ait-desktop-log-"));
  homes.push(home);
  const logPath = path.join(home, "daemon.log");
  writeFileSync(logPath, "small");
  rotateDaemonLog(logPath, 16);
  expect(readFileSync(logPath, "utf8")).toBe("small");

  writeFileSync(logPath, "x".repeat(32));
  rotateDaemonLog(logPath, 16);
  expect(existsSync(logPath)).toBe(false);
  expect(readFileSync(`${logPath}.1`, "utf8")).toBe("x".repeat(32));
  expect(() => rotateDaemonLog(path.join(home, "missing.log"), 16)).not.toThrow();
});

describe.skipIf(!process.env.AIT_SERVER_BIN)("real Rust child lifecycle", () => {
  it("reads saved settings on each start, applies fixed ports, and lets the environment override them", async () => {
    let listen = "0.0.0.0:0";
    const manager = create(process.env.AIT_SERVER_BIN!, undefined, {
      getListen: async () => listen,
    });
    const initial = await manager.start();
    expect(initial.listen).toMatch(/^0\.0\.0\.0:\d+$/);
    expect(initial.connectAddress).toBe(initial.listen!.replace("0.0.0.0", "127.0.0.1"));
    expect(manager.authorization(`ws://${initial.connectAddress}/v1/ws`)).toHaveLength(64);
    expect(manager.authorization(`ws://${initial.listen}/v1/ws`)).toBeUndefined();
    listen = `127.0.0.1:${initial.listen!.split(":").at(-1)}`;
    expect((await manager.start()).listen).toBe(initial.listen);
    const restarted = await manager.restart();
    expect(restarted.listen).toBe(listen);
    expect(restarted.serverId).toBe(initial.serverId);
    await manager.stop();
    const fresh = create(process.env.AIT_SERVER_BIN!, initial.home, {
      getListen: async () => listen,
    });
    expect((await fresh.start()).listen).toBe(listen);
    await fresh.stop();
    const overridden = create(process.env.AIT_SERVER_BIN!, initial.home, {
      listen: "[::]:0",
      getListen: async () => listen,
    });
    const status = await overridden.start();
    expect(status.listenOverride).toBe("[::]:0");
    expect(status.connectAddress).toMatch(/^\[::1\]:\d+$/);
  }, 60000);

  it("connects a concrete LAN listener without lending its token to localhost", async ({
    skip,
  }) => {
    const address = Object.values(networkInterfaces())
      .flat()
      .find((item) => item && item.family === "IPv4" && !item.internal)?.address;
    if (!address) return skip();
    const manager = create(process.env.AIT_SERVER_BIN!, undefined, {
      listen: `${address}:0`,
    });
    const status = await manager.start();
    expect(status.connectAddress).toBe(status.listen);
    expect(manager.authorization(`ws://${status.connectAddress}/v1/ws`)).toHaveLength(64);
    expect(
      manager.authorization(`ws://localhost:${status.listen!.split(":").at(-1)}/v1/ws`),
    ).toBeUndefined();
  }, 30000);

  it("serializes startup, authenticates readiness, restores credentials and cleans up on restart/stop", async () => {
    const manager = create(process.env.AIT_SERVER_BIN!);
    const [a, b] = await Promise.all([manager.start(), manager.start()]);
    expect(a).toEqual(b);
    expect(a.status).toBe("running");
    expect(a.serverId).not.toBe("");
    expect(a.version).toBe(expectedVersion);
    const token = manager.authorization(`ws://${a.listen}/v1/ws`)!;
    expect(token).toHaveLength(64);
    expect(manager.authorization(`ws://localhost:${a.listen!.split(":").at(-1)}/v1/ws`)).toBe(
      token,
    );
    expect(manager.authorization(`ws://${a.listen}/v1/ws?token=x`)).toBeUndefined();
    expect(manager.authorization("ws://127.0.0.1:1/v1/ws")).toBeUndefined();
    expect(JSON.stringify(a)).not.toContain(token);
    expect(readFileSync(path.join(a.home, "daemon.log"), "utf8")).not.toContain(token);
    await expect(manager.stop({ pid: a.pid! + 1, startedAt: a.startedAt! })).rejects.toThrow(
      "changed",
    );
    expect(manager.status().status).toBe("running");
    const restarted = await manager.restart();
    expect(restarted.serverId).toBe(a.serverId);
    expect(restarted.version).toBe(expectedVersion);
    expect(restarted.listen).toBe(a.listen);
    expect(restarted.pid).not.toBe(a.pid);
    expect(manager.authorization(`ws://${restarted.listen}/v1/ws`)).toBe(token);
    expect(() => process.kill(a.pid!, 0)).toThrow();
    await manager.stop();
    expect(manager.status()).toMatchObject({ status: "stopped", version: null });
    expect(() => process.kill(restarted.pid!, 0)).toThrow();
    expect(manager.authorization(`ws://${restarted.listen}/v1/ws`)).toBeUndefined();
  }, 40000);

  it("uses a saved API token across Desktop restarts and honors an explicit environment override", async () => {
    vi.stubEnv("AIT_SERVER_TOKEN", undefined);
    const manager = create(process.env.AIT_SERVER_BIN!);
    const initial = await manager.start();
    const token = readFileSync(path.join(initial.home, "local-api.token"), "utf8");
    expect(manager.authorization(`ws://${initial.connectAddress}/v1/ws`)).toBe(token);
    expect(statSync(path.join(initial.home, "local-api.token")).mode & 0o077).toBe(0);
    const restarted = await manager.restart();
    expect(manager.authorization(`ws://${restarted.connectAddress}/v1/ws`)).toBe(token);
    await manager.stop();
    const override = "explicit-environment-token-at-least-32-characters";
    vi.stubEnv("AIT_SERVER_TOKEN", override);
    const external = await manager.start();
    expect(manager.authorization(`ws://${external.connectAddress}/v1/ws`)).toBe(override);
    expect(readFileSync(path.join(initial.home, "local-api.token"), "utf8")).toBe(token);
    expect(readFileSync(path.join(initial.home, "daemon.log"), "utf8")).not.toContain(override);
  }, 60000);

  it("disconnects the shared server authorization and requires a fresh login after restart", async () => {
    const manager = create(process.env.AIT_SERVER_BIN!);
    await expect(manager.hubRequest("GET")).rejects.toThrow("The local runtime is not ready.");
    await manager.start();
    await expect(manager.hubRequest("GET")).resolves.toMatchObject({
      online: false,
      connecting: false,
      management: { mode: "managed", autonomous: true },
    });
    await expect(
      manager.hubRequest("PUT", {
        center_url: "http://example.com",
        hub_ticket: "a".repeat(64),
        node_session_id: "00000000-0000-4000-8000-000000000001",
      }),
    ).rejects.toMatchObject({ status: 409 });
    await expect(manager.hubRequest("DELETE")).resolves.toBeNull();
    await expect(manager.hubRequest("GET")).resolves.toMatchObject({
      management: { phase: "disconnected", binding: null },
    });
    await expect(manager.hubRequest("PUT", {})).rejects.toMatchObject({ status: 409 });
    await manager.restart();
    await expect(manager.hubRequest("GET")).resolves.toMatchObject({
      management: { phase: "disconnected", binding: null },
    });
    await expect(manager.relayRequest("DELETE")).resolves.toBeNull();
  }, 40000);

  it("does not adopt or stop another process using the same data directory", async () => {
    const first = create(process.env.AIT_SERVER_BIN!);
    const running = await first.start();
    const second = create(process.env.AIT_SERVER_BIN!, running.home);
    await expect(second.start()).rejects.toThrow();
    await second.stop();
    expect(() => process.kill(running.pid!, 0)).not.toThrow();
    expect(first.status().status).toBe("running");
  }, 30000);

  it("keeps a retained daemon running for the next session and stops it from there", async () => {
    const first = create(process.env.AIT_SERVER_BIN!);
    const running = await first.start();
    const token = first.authorization(`ws://${running.connectAddress}/v1/ws`);
    await expect(first.retainForNextSession()).resolves.toBe(true);
    // The handed-over daemon must not be stopped by the session that launched it.
    await first.stop();
    expect(() => process.kill(running.pid!, 0)).not.toThrow();
    const record = readFileSync(path.join(running.home, "desktop-daemon.json"), "utf8");
    expect(statSync(path.join(running.home, "desktop-daemon.json")).mode & 0o077).toBe(0);
    expect(record).toContain(token!);

    const next = create(process.env.AIT_SERVER_BIN!, running.home);
    const reattached = await next.start();
    expect(reattached).toMatchObject({
      status: "running",
      pid: running.pid,
      serverId: running.serverId,
      listen: running.listen,
      ownedByDesktop: true,
    });
    expect(next.authorization(`ws://${reattached.connectAddress}/v1/ws`)).toBe(token);
    expect(existsSync(path.join(running.home, "desktop-daemon.json"))).toBe(false);

    await next.stop();
    expect(() => process.kill(running.pid!, 0)).toThrow();
    expect(existsSync(path.join(running.home, "desktop-daemon.json"))).toBe(false);
  }, 60000);

  it("restarts a retained daemon whose listen setting changed while Ait was closed", async () => {
    let listen = "127.0.0.1:0";
    const first = create(process.env.AIT_SERVER_BIN!, undefined, { getListen: async () => listen });
    const running = await first.start();
    await first.retainForNextSession();
    listen = "[::1]:0";

    const next = create(process.env.AIT_SERVER_BIN!, running.home, {
      getListen: async () => listen,
    });
    const restarted = await next.start();
    expect(restarted.pid).not.toBe(running.pid);
    expect(restarted.connectAddress).toMatch(/^\[::1\]:\d+$/);
    expect(() => process.kill(running.pid!, 0)).toThrow();
  }, 60000);
});

it("preserves an unverified live PID without adopting or signalling it", async () => {
  const home = mkdtempSync(path.join(tmpdir(), "ait-desktop-retained-"));
  homes.push(home);
  const record = path.join(home, "desktop-daemon.json");
  const unrelated = spawn(process.execPath, ["-e", "setTimeout(() => {}, 30000)"]);
  try {
    writeFileSync(
      record,
      JSON.stringify({
        pid: unrelated.pid,
        token: "f".repeat(64),
        listen: "127.0.0.1:1",
        configuredListen: "127.0.0.1:0",
        startedAt: new Date().toISOString(),
      }),
    );
    const manager = create("/missing/ait-server", home);
    // The PID is alive but does not answer with the saved token, so it is never adopted.
    await expect(manager.start()).rejects.toThrow();
    expect(unrelated.exitCode).toBeNull();
    expect(unrelated.signalCode).toBeNull();
    expect(existsSync(record)).toBe(true);
    expect(manager.status().ownedByDesktop).toBe(false);
  } finally {
    unrelated.kill();
  }
});

it("keeps Rust desktop data separate from the legacy Paseo home", () => {
  expect(resolveDesktopDaemonHome({ PASEO_HOME: "/legacy-paseo" })).not.toBe("/legacy-paseo");
  expect(
    resolveDesktopDaemonHome({ AIT_SERVER_DATA_DIR: "/rust-desktop", PASEO_HOME: "/legacy-paseo" }),
  ).toBe("/rust-desktop");
});

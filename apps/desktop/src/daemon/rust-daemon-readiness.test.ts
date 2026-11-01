import { existsSync, mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RustDaemonManager } from "./rust-daemon";

interface ReadinessConfig {
  busy?: boolean;
  silent?: boolean;
  closeHello?: boolean;
  rejectHello?: boolean;
  waitForReady?: boolean;
  exitBeforeReady?: boolean;
  wrongPid?: boolean;
}

const fixtures: ReturnType<typeof create>[] = [];

beforeEach(() => vi.stubEnv("AIT_SERVER_TOKEN", undefined));

function create(config: ReadinessConfig = {}) {
  const home = mkdtempSync(path.join(tmpdir(), "ait-readiness-test-"));
  const binary = path.join(home, "daemon");
  const script = fileURLToPath(new URL("./fixtures/readiness-daemon.cjs", import.meta.url));
  writeFileSync(binary, `#!${process.execPath}\nrequire(${JSON.stringify(script)});\n`, {
    mode: 0o700,
  });
  const configure = (next: ReadinessConfig) => {
    const file = path.join(home, "readiness.json");
    writeFileSync(`${file}.tmp`, JSON.stringify(next));
    renameSync(`${file}.tmp`, file);
  };
  configure(config);
  const managers: RustDaemonManager[] = [];
  const manager = (executable = binary) => {
    const instance = new RustDaemonManager({
      binary: executable,
      home,
      listen: "127.0.0.1:0",
      timeoutMs: 800,
    });
    managers.push(instance);
    return instance;
  };
  const lines = (name: string) =>
    existsSync(path.join(home, name))
      ? readFileSync(path.join(home, name), "utf8").trim().split("\n")
      : [];
  const fixture = { home, configure, manager, managers, lines };
  fixtures.push(fixture);
  return fixture;
}

afterEach(async () => {
  for (const fixture of fixtures.splice(0)) {
    fixture.configure({});
    for (const manager of fixture.managers) await manager.stop();
    // Also clean up handed-over processes when an assertion fails before reattachment.
    for (const pid of fixture.lines("pids").map(Number)) {
      try {
        process.kill(pid, "SIGKILL");
      } catch {
        /* Already exited. */
      }
    }
    rmSync(fixture.home, { recursive: true, force: true });
  }
  vi.unstubAllEnvs();
});

describe.skipIf(process.platform === "win32")("daemon readiness and process ownership", () => {
  it("uses the authenticated handshake without querying busy business services", async () => {
    const fixture = create({ busy: true });
    const manager = fixture.manager();
    await expect(manager.start()).resolves.toMatchObject({ status: "running" });
    expect(fixture.lines("requests")).toEqual([]);
    expect(fixture.lines("terminations")).toEqual([]);
  });

  it.each([{ silent: true }, { closeHello: true }, { rejectHello: true }, { waitForReady: true }])(
    "keeps the child after failed readiness %j and reconnects to the same PID",
    async (config) => {
      const fixture = create(config);
      const manager = fixture.manager();
      await expect(manager.start()).rejects.toThrow();
      const pid = Number(fixture.lines("pids")[0]);
      expect(() => process.kill(pid, 0)).not.toThrow();
      expect(manager.status()).toMatchObject({ status: "errored", pid, ownedByDesktop: true });
      expect(fixture.lines("terminations")).toEqual([]);
      fixture.configure({});
      const [first, second] = await Promise.all([manager.start(), manager.start()]);
      expect(first).toMatchObject({ status: "running", pid, error: null });
      expect(second).toEqual(first);
      expect(fixture.lines("pids")).toEqual([String(pid)]);
      expect(manager.authorization(`ws://${first.connectAddress}/v1/ws`)).toHaveLength(64);
    },
  );

  it("still stops an unresponsive child when explicitly requested", async () => {
    const fixture = create({ silent: true });
    const manager = fixture.manager();
    await expect(manager.start()).rejects.toThrow();
    const pid = Number(fixture.lines("pids")[0]);
    await expect(manager.stop()).resolves.toMatchObject({ status: "stopped", pid: null });
    expect(fixture.lines("terminations")).toEqual([String(pid)]);
    expect(() => process.kill(pid, 0)).toThrow();
  });

  it("replaces an unresponsive child only on explicit restart", async () => {
    const fixture = create({ silent: true });
    const manager = fixture.manager();
    await expect(manager.start()).rejects.toThrow();
    const pid = Number(fixture.lines("pids")[0]);
    fixture.configure({});
    const next = await manager.restart();
    expect(next.status).toBe("running");
    expect(next.pid).not.toBe(pid);
    expect(fixture.lines("pids")).toHaveLength(2);
    expect(fixture.lines("terminations")).toEqual([String(pid)]);
  });

  it.each([{ silent: true }, { waitForReady: true }])(
    "hands over a live child after a readiness timeout %j",
    async (config) => {
      const fixture = create(config);
      const first = fixture.manager();
      await expect(first.start()).rejects.toThrow();
      const pid = Number(fixture.lines("pids")[0]);
      await expect(first.retainForNextSession()).resolves.toBe(true);
      await first.stop();
      expect(() => process.kill(pid, 0)).not.toThrow();
      fixture.configure({});
      await expect(fixture.manager().start()).resolves.toMatchObject({ status: "running", pid });
      expect(fixture.lines("pids")).toHaveLength(1);
    },
  );

  it.each([{ busy: true }, { silent: true }, { closeHello: true }, { rejectHello: true }])(
    "preserves the handover record on failed reattachment %j without spawning",
    async (config) => {
      const fixture = create();
      const first = fixture.manager();
      const running = await first.start();
      await first.retainForNextSession();
      fixture.configure(config);
      const next = fixture.manager();
      await expect(next.start()).rejects.toThrow();
      expect(existsSync(path.join(fixture.home, "desktop-daemon.json"))).toBe(true);
      expect(fixture.lines("pids")).toHaveLength(1);
      expect(() => process.kill(running.pid!, 0)).not.toThrow();
      expect(next.status().ownedByDesktop).toBe(false);
      fixture.configure({});
      await expect(next.start()).resolves.toMatchObject({ status: "running", pid: running.pid });
      expect(existsSync(path.join(fixture.home, "desktop-daemon.json"))).toBe(false);
    },
  );

  it("discards a record whose authenticated PID differs without signalling that process", async () => {
    const fixture = create();
    const first = fixture.manager();
    const running = await first.start();
    await first.retainForNextSession();
    fixture.configure({ wrongPid: true });
    const next = fixture.manager("/missing/ait-server");
    await expect(next.start()).rejects.toThrow();
    expect(next.status().ownedByDesktop).toBe(false);
    expect(existsSync(path.join(fixture.home, "desktop-daemon.json"))).toBe(false);
    expect(fixture.lines("terminations")).toEqual([]);
    expect(() => process.kill(running.pid!, 0)).not.toThrow();
  });

  it("discards a dead retained PID and starts a new daemon", async () => {
    const fixture = create();
    const first = fixture.manager();
    const running = await first.start();
    await first.retainForNextSession();
    process.kill(running.pid!, "SIGTERM");
    await expect
      .poll(() => {
        try {
          process.kill(running.pid!, 0);
          return true;
        } catch {
          return false;
        }
      })
      .toBe(false);
    const next = await fixture.manager().start();
    expect(next.status).toBe("running");
    expect(next.pid).not.toBe(running.pid);
    expect(fixture.lines("pids")).toHaveLength(2);
    expect(existsSync(path.join(fixture.home, "desktop-daemon.json"))).toBe(false);
  });

  it("reports a real child exit without retaining ownership and can start a new child", async () => {
    const fixture = create({ exitBeforeReady: true });
    const manager = fixture.manager();
    await expect(manager.start()).rejects.toThrow();
    expect(manager.status()).toMatchObject({ status: "errored", pid: null, ownedByDesktop: false });
    fixture.configure({});
    await expect(manager.start()).resolves.toMatchObject({ status: "running" });
    expect(fixture.lines("pids")).toHaveLength(2);
  });
});

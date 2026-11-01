import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it } from "vitest";
import { RustDaemonManager } from "./rust-daemon";

interface RpcError {
  code: string;
  retryable: boolean;
}

const busy: RpcError = { code: "resource_exhausted", retryable: true };
const fixtures: { manager: RustDaemonManager; home: string }[] = [];

function create(
  errors: (RpcError | null)[],
  options: {
    timeoutMs?: number;
    repeatLastError?: boolean;
    rejectHello?: boolean;
    closeAfterError?: boolean;
  } = {},
) {
  const home = mkdtempSync(path.join(tmpdir(), "ait-readiness-test-"));
  const binary = path.join(home, "daemon");
  const fixture = fileURLToPath(new URL("./fixtures/readiness-daemon.cjs", import.meta.url));
  writeFileSync(binary, `#!${process.execPath}\nrequire(${JSON.stringify(fixture)});\n`, {
    mode: 0o700,
  });
  writeFileSync(path.join(home, "readiness.json"), JSON.stringify({ errors, ...options }));
  const manager = new RustDaemonManager({
    binary,
    home,
    listen: "127.0.0.1:0",
    timeoutMs: options.timeoutMs ?? 3000,
  });
  fixtures.push({ manager, home });
  return {
    manager,
    home,
    requests: (): { method: string }[] =>
      existsSync(path.join(home, "requests.json"))
        ? JSON.parse(readFileSync(path.join(home, "requests.json"), "utf8"))
        : [],
    pid: () => Number(readFileSync(path.join(home, "daemon.pid"), "utf8")),
  };
}

afterEach(async () => {
  for (const { manager, home } of fixtures.splice(0)) {
    await manager.stop();
    rmSync(home, { recursive: true, force: true });
  }
});

describe.skipIf(process.platform === "win32")("daemon readiness under resource contention", () => {
  it("retries busy status replies without terminating the daemon", async () => {
    const fixture = create([busy, busy]);
    await expect(fixture.manager.start()).resolves.toMatchObject({ status: "running" });
    expect(fixture.requests()).toHaveLength(3);
    expect(fixture.requests().every(({ method }) => method === "daemon.get_status.request")).toBe(
      true,
    );
    expect(fixture.manager.status().pid).toBe(fixture.pid());
    expect(existsSync(path.join(fixture.home, "termination"))).toBe(false);
    expect(() => process.kill(fixture.pid(), 0)).not.toThrow();
  });

  it("keeps the original deadline when status stays busy and stops the child", async () => {
    const fixture = create([busy], { repeatLastError: true, timeoutMs: 1000 });
    const started = Date.now();
    await expect(fixture.manager.start()).rejects.toThrow("handshake timed out");
    expect(Date.now() - started).toBeLessThan(2500);
    expect(fixture.requests().length).toBeGreaterThan(1);
    expect(fixture.manager.status().status).toBe("errored");
    expect(() => process.kill(fixture.pid(), 0)).toThrow();
  });

  it.each([
    { code: "forbidden", retryable: false },
    { code: "resource_exhausted", retryable: false },
    { code: "server_draining", retryable: true },
  ])("does not retry $code with retryable=$retryable", async (error) => {
    const fixture = create([error]);
    await expect(fixture.manager.start()).rejects.toThrow("rejected daemon.get_status.request");
    expect(fixture.requests()).toHaveLength(1);
    expect(() => process.kill(fixture.pid(), 0)).toThrow();
  });

  it("does not retry an error before the authenticated handshake completes", async () => {
    const fixture = create([busy], { rejectHello: true });
    await expect(fixture.manager.start()).rejects.toThrow("rejected daemon.get_status.request");
    expect(fixture.requests()).toHaveLength(0);
    expect(() => process.kill(fixture.pid(), 0)).toThrow();
  });

  it("cancels a pending retry when the connection closes", async () => {
    const fixture = create([busy], { closeAfterError: true });
    await expect(fixture.manager.start()).rejects.toThrow("closed before readiness handshake");
    expect(fixture.requests()).toHaveLength(1);
    expect(() => process.kill(fixture.pid(), 0)).toThrow();
  });
});
